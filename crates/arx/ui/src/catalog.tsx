import {Fragment, type ReactNode} from 'react';
import {createComponentImplementation, type ReactCatalogComponent} from '@a2ui/react';
import {Catalog} from '@a2ui/web_core';
import {basicCatalog} from '@a2ui/web_core/catalogs/basic/v1';

type ChildRef = {id: string} | string;

function childId(child: ChildRef | undefined): string | undefined {
  if (!child) return undefined;
  return typeof child === 'string' ? child : child.id;
}

function children(node: unknown, buildChild: (id: string) => ReactNode): ReactNode {
  if (!Array.isArray(node)) return null;
  return node.map((child, index) => {
    const id = childId(child as ChildRef);
    return id ? <Fragment key={`${id}-${index}`}>{buildChild(id)}</Fragment> : null;
  });
}

function single(child: unknown, buildChild: (id: string) => ReactNode): ReactNode {
  const id = childId(child as ChildRef);
  return id ? buildChild(id) : null;
}

const custom = {
  Text: createComponentImplementation(
    basicCatalog.components.get('Text')!,
    ({props}) => (
      <span
        className={`a2ui-text ${props.variant === 'caption' ? 'a2ui-caption' : ''}`}
        style={{flexGrow: Number(props.weight ?? 0)}}
      >
        {String(props.text ?? '')}
      </span>
    ),
  ),
  Row: createComponentImplementation(
    basicCatalog.components.get('Row')!,
    ({props, buildChild}) => (
      <div
        className="a2ui-row"
        style={{justifyContent: justifyContent(props.justify), alignItems: alignItems(props.align)}}
      >
        {children(props.children, buildChild)}
      </div>
    ),
  ),
  Column: createComponentImplementation(
    basicCatalog.components.get('Column')!,
    ({props, buildChild}) => (
      <div
        className="a2ui-column"
        style={{justifyContent: justifyContent(props.justify), alignItems: alignItems(props.align)}}
      >
        {children(props.children, buildChild)}
      </div>
    ),
  ),
  Card: createComponentImplementation(
    basicCatalog.components.get('Card')!,
    ({props, buildChild}) => (
      <div className="a2ui-card" style={{flexGrow: Number(props.weight ?? 0)}}>
        {single(props.child, buildChild)}
      </div>
    ),
  ),
  List: createComponentImplementation(
    basicCatalog.components.get('List')!,
    ({props, buildChild}) => (
      <div
        className={props.direction === 'horizontal' ? 'a2ui-list horizontal' : 'a2ui-list'}
        style={{flexGrow: Number(props.weight ?? 0)}}
      >
        {children(props.children, buildChild)}
      </div>
    ),
  ),
  Button: createComponentImplementation(
    basicCatalog.components.get('Button')!,
    ({props, buildChild}) => (
      <button
        className={`a2ui-button ${props.variant === 'primary' ? 'primary' : ''}`}
        disabled={props.isValid === false}
        onClick={() => props.action?.()}
        type="button"
      >
        {single(props.child, buildChild)}
      </button>
    ),
  ),
};

function justifyContent(value: unknown): string {
  return {
    start: 'flex-start',
    center: 'center',
    end: 'flex-end',
    spaceBetween: 'space-between',
    spaceAround: 'space-around',
    spaceEvenly: 'space-evenly',
    stretch: 'stretch',
  }[String(value)] ?? 'flex-start';
}

function alignItems(value: unknown): string {
  return {
    start: 'flex-start',
    center: 'center',
    end: 'flex-end',
    stretch: 'stretch',
  }[String(value)] ?? 'stretch';
}

const components = new Map<string, ReactCatalogComponent>(basicCatalog.components);
for (const [name, implementation] of Object.entries(custom)) {
  components.set(name, implementation);
}

export const catalog = new Catalog<ReactCatalogComponent>(
  basicCatalog.id,
  '1.0',
  [...components.values()],
  [...basicCatalog.functions.values()],
);
