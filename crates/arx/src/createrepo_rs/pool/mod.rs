//! Thread pool for parallel package processing.
//!
//! This module provides a worker pool for processing jobs in parallel using
//! `std::thread` and crossbeam-channel.

use crate::createrepo_rs::rpm::RpmReader;
use crate::createrepo_rs::types::{
    ChangelogEntry, ChecksumType, Dependency, Package as TypesPackage,
    PackageFile as TypesPackageFile,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

/// Job types that can be submitted to the worker pool.
#[derive(Debug, Clone)]
pub enum Job {
    /// Process a package at the given path.
    ProcessPackage(PathBuf),
    /// Placeholder for other job types.
    Other(String),
}

impl Job {
    /// Returns the path associated with this job, if any.
    #[must_use]
    pub const fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::ProcessPackage(p) => Some(p),
            Self::Other(_) => None,
        }
    }
}

/// Result of processing a job.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ProcessingResult {
    /// Successful processing of a package with parsed metadata.
    Success(PathBuf, TypesPackage),
    /// Error during processing.
    Error(PathBuf, String),
}

/// Internal message passed between pool and workers.
enum WorkerMessage {
    Job(Job),
    Stop,
}

/// Worker pool for parallel job processing.
///
/// Submits jobs to worker threads that process them in parallel.
pub struct WorkerPool {
    workers: Vec<thread::JoinHandle<()>>,
    job_sender: Option<Sender<WorkerMessage>>,
    shutdown_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    result_sender: Option<Sender<ProcessingResult>>,
}

impl WorkerPool {
    /// Creates a new worker pool with the specified number of workers.
    #[must_use]
    pub fn new(num_workers: usize) -> (Self, Receiver<ProcessingResult>) {
        let (job_sender, job_receiver) = bounded::<WorkerMessage>(num_workers * 256);
        let (result_sender, result_receiver) = bounded::<ProcessingResult>(num_workers * 256);
        let shutdown_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut workers = Vec::with_capacity(num_workers);

        for id in 0..num_workers {
            let receiver = job_receiver.clone();
            let result_sender = result_sender.clone();
            let shutdown_flag = shutdown_flag.clone();

            let worker = thread::Builder::new()
                .name(format!("worker-{id}"))
                .spawn(move || {
                    Self::worker_loop(id, receiver, result_sender, shutdown_flag);
                })
                .unwrap_or_else(|e| {
                    panic!("failed to spawn worker thread: {e}");
                });

            workers.push(worker);
        }

        let pool = Self {
            workers,
            job_sender: Some(job_sender),
            shutdown_flag,
            result_sender: Some(result_sender),
        };

        (pool, result_receiver)
    }

    /// Closes all channels and signals workers to exit.
    pub fn close(&mut self) {
        self.shutdown_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.job_sender = None;
        self.result_sender = None;
    }

    /// Submits a job to the pool for processing.
    ///
    /// Returns true if the job was submitted successfully, false if the pool
    /// has been shut down or the submit timed out (workers likely stuck).
    #[must_use]
    pub fn submit(&self, job: Job) -> bool {
        if let Some(ref sender) = self.job_sender {
            sender
                .send_timeout(WorkerMessage::Job(job), Duration::from_secs(30))
                .is_ok()
        } else {
            false
        }
    }

    /// Signals all workers to stop processing.
    pub fn shutdown(&self) {
        self.shutdown_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(ref sender) = self.job_sender {
            for _ in &self.workers {
                let _ = sender.send(WorkerMessage::Stop);
            }
        }
    }

    /// Waits for all workers to complete and consumes the pool.
    pub fn join(mut self) {
        self.close();

        use std::mem;
        let mut workers = Vec::new();
        mem::swap(&mut workers, &mut self.workers);

        for worker in workers {
            let _ = worker.join();
        }
    }

    /// The main loop for each worker thread.
    fn worker_loop(
        _id: usize,
        receiver: Receiver<WorkerMessage>,
        result_sender: Sender<ProcessingResult>,
        shutdown_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        while !shutdown_flag.load(std::sync::atomic::Ordering::SeqCst) {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(WorkerMessage::Job(job)) => {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        Self::process_job(job)
                    }));
                    let result = match result {
                        Ok(r) => r,
                        Err(e) => {
                            let msg = if let Some(s) = e.downcast_ref::<String>() {
                                s.clone()
                            } else if let Some(s) = e.downcast_ref::<&str>() {
                                (*s).to_string()
                            } else {
                                "worker panicked".to_string()
                            };
                            ProcessingResult::Error(PathBuf::from("panic"), msg)
                        }
                    };
                    let _ = result_sender.send(result);
                }
                Ok(WorkerMessage::Stop)
                | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    break;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Processes a single job and returns the result.
    fn process_job(job: Job) -> ProcessingResult {
        match job {
            Job::ProcessPackage(path) => {
                let mut reader = match RpmReader::open(&path) {
                    Ok(r) => r,
                    Err(e) => {
                        return ProcessingResult::Error(path, format!("Failed to open RPM: {e}"));
                    }
                };

                let rpm_pkg = match reader.read_package() {
                    Ok(p) => p,
                    Err(e) => {
                        return ProcessingResult::Error(
                            path,
                            format!("Failed to read package: {e}"),
                        );
                    }
                };

                let pkg = convert_package(rpm_pkg);
                ProcessingResult::Success(path, pkg)
            }
            Job::Other(_msg) => ProcessingResult::Success(PathBuf::new(), TypesPackage::default()),
        }
    }
}

/// Converts an `rpm::Package` to a `types::Package`.
fn convert_package(rpm_pkg: crate::createrepo_rs::rpm::Package) -> TypesPackage {
    let location = rpm_pkg.location;
    TypesPackage {
        pkgid: rpm_pkg.sha256.clone(),
        name: rpm_pkg.name,
        arch: rpm_pkg.arch,
        version: rpm_pkg.version,
        epoch: rpm_pkg.epoch.and_then(|e| e.parse().ok()),
        release: rpm_pkg.release,
        filename: location.clone(),
        location: location.clone(),
        checksum_type: ChecksumType::Sha256,
        checksum: rpm_pkg.sha256,
        source_pkg: rpm_pkg.sourcerpm.clone(),
        size_archive: rpm_pkg.file_size,
        size_installed: 0,
        size_package: rpm_pkg.size,
        time_file: rpm_pkg.time_file,
        time_build: rpm_pkg.time_build,
        summary: rpm_pkg.summary,
        description: rpm_pkg.description,
        packager: rpm_pkg.packager,
        url: rpm_pkg.url,
        license: rpm_pkg.license,
        vendor: rpm_pkg.vendor,
        group: rpm_pkg.group,
        buildhost: rpm_pkg.buildhost,
        sourcerpm: rpm_pkg.sourcerpm,
        requires: rpm_pkg.requires.into_iter().map(Dependency::from).collect(),
        provides: rpm_pkg.provides.into_iter().map(Dependency::from).collect(),
        conflicts: rpm_pkg
            .conflicts
            .into_iter()
            .map(Dependency::from)
            .collect(),
        obsoletes: rpm_pkg
            .obsoletes
            .into_iter()
            .map(Dependency::from)
            .collect(),
        suggests: rpm_pkg.suggests.into_iter().map(Dependency::from).collect(),
        enhances: rpm_pkg.enhances.into_iter().map(Dependency::from).collect(),
        recommends: rpm_pkg
            .recommends
            .into_iter()
            .map(Dependency::from)
            .collect(),
        supplements: rpm_pkg
            .supplements
            .into_iter()
            .map(Dependency::from)
            .collect(),
        files: rpm_pkg
            .files
            .into_iter()
            .map(|f| TypesPackageFile {
                path: f.path,
                file_type: f.file_type.unwrap_or_default(),
                digest: f.digest,
                size: 0,
            })
            .collect(),
        changelogs: rpm_pkg
            .changelogs
            .into_iter()
            .map(ChangelogEntry::from)
            .collect(),
        location_href: Some(location),
        header_start: None,
        header_end: None,
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}
