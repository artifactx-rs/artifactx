import {useEffect, useRef, useState} from 'react';
import {
  A2uiSurface,
  MarkdownComponentContext,
  type MarkdownComponentProps,
  type ReactCatalogComponent,
} from '@a2ui/react';
import {MessageProcessor} from '@a2ui/web_core';
import type {ActionPayload, SurfaceModel} from '@a2ui/web_core/state';
import {basicCatalog} from '@a2ui/web_core/catalogs/basic/v1';

function PlainMarkdown({markdown}: MarkdownComponentProps) {
  return <span>{markdown}</span>;
}

const navigation = ['Overview', 'Packages', 'Publish', 'Cutover', 'Settings'];

export default function App() {
  const [error, setError] = useState<string | null>(null);
  const [processor] = useState(() => {
    const p = new MessageProcessor<ReactCatalogComponent>(
      [basicCatalog],
      async (action: ActionPayload) => {
        const query = typeof action.context?.query === 'string' ? action.context.query : '';
        try {
          const response = await fetch(`/api/v1/ui/search?q=${encodeURIComponent(query)}`);
          if (!response.ok) {
            throw new Error(`search failed: ${response.status}`);
          }
          setError(null);
          p.processMessages(await response.json());
        } catch (reason) {
          setError(String(reason));
        }
      },
    );
    return p;
  });
  const [surfaces, setSurfaces] = useState<SurfaceModel<ReactCatalogComponent>[]>([]);
  const loaded = useRef(false);

  useEffect(() => {
    const sync = () => setSurfaces(Array.from(processor.model.surfacesMap.values()));
    const created = processor.onSurfaceCreated(sync);
    const deleted = processor.onSurfaceDeleted(sync);

    if (!loaded.current) {
      loaded.current = true;
      fetch('/api/v1/ui/overview')
        .then(async response => {
          if (!response.ok) {
            throw new Error(`overview failed: ${response.status}`);
          }
          return response.json();
        })
        .then(messages => {
          processor.processMessages(messages);
          sync();
          setError(null);
        })
        .catch(reason => setError(String(reason)));
    }

    return () => {
      created.unsubscribe();
      deleted.unsubscribe();
    };
  }, [processor]);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">△</span>
          <span>ArtifactX</span>
        </div>
        <nav className="nav" aria-label="Console sections">
          {navigation.map((item, index) => (
            <button
              className={index === 0 ? 'nav-item active' : 'nav-item'}
              disabled={index !== 0}
              key={item}
              title={index === 0 ? undefined : 'Available in a later slice'}
              type="button"
            >
              <span className="nav-index">0{index + 1}</span>
              {item}
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <span className="live-dot" />
          Read-only preview
        </div>
      </aside>
      <main className="main">
        <header className="topbar">
          <div className="crumbs">
            <span className="mono">arx</span>
            <span className="slash">/</span>
            <span>console</span>
          </div>
          <div className="topbar-right">
            <span className="pill">API v1</span>
            <span className="live">
              <span className="live-dot" />
              Live
            </span>
          </div>
        </header>
        <section className="surface">
          {error ? <div className="state error">{error}</div> : null}
          {!error && surfaces.length === 0 ? <div className="state">Loading repository surface…</div> : null}
          <MarkdownComponentContext.Provider value={PlainMarkdown}>
            {surfaces.map(surface => (
              <A2uiSurface key={surface.id} surface={surface} />
            ))}
          </MarkdownComponentContext.Provider>
        </section>
      </main>
    </div>
  );
}
