import { useEffect, useMemo, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { BookOpen, Search, X } from 'lucide-react';
import { useIdentities, useMemory, useMemorySearch } from '../api/queries';
import { ContextDrawer } from '../components/ContextDrawer';
import { ReferenceChip } from '../components/ReferenceChip';
import { Status } from '../components/Status';
import {
  BlockSkeleton,
  EmptyState,
  ErrorState,
  PageHeader,
} from '../components/ui';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { formatDateTime, formatRelative, plural } from '../lib/format';
import { SafeMarkdown } from '../lib/markdown';

export function MemoryPage() {
  const { id } = useParams<{ id: string }>();
  const memory = useMemory(id);
  const identities = useIdentities(id);
  const [search, setSearch] = useState('');
  const debouncedSearch = useDebouncedValue(search, 300);
  const trimmedQuery = debouncedSearch.trim();
  const searchResults = useMemorySearch(id, trimmedQuery);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [contextOpen, setContextOpen] = useState(false);

  const activeQuery = trimmedQuery.length > 0;
  const active = activeQuery ? searchResults : memory;
  const pages = useMemo(() => active.data ?? [], [active.data]);

  useEffect(() => {
    if (pages.length === 0) {
      setSelectedId(null);
      return;
    }
    setSelectedId((current) =>
      current && pages.some((page) => page.id === current) ? current : pages[0].id,
    );
  }, [pages]);

  const selected = pages.find((page) => page.id === selectedId) ?? null;
  const loading = active.isLoading;
  const error = active.isError ? active.error : null;

  return (
    <>
      <PageHeader
        meta={`memory${memory.data ? ` · ${memory.data.length} ${plural(memory.data.length, 'page')}` : ''}`}
        title="Memory"
        description="Persistent assessment knowledge, versioned and referenced."
        actions={
          <button
            type="button"
            className="btn"
            onClick={() => setContextOpen(true)}
          >
            <BookOpen size={14} strokeWidth={1.5} aria-hidden="true" />
            Build context
          </button>
        }
      />

      <div className="memory-layout">
        <aside className="memory-list" aria-label="Memory pages">
          <div className="memory-search">
            <div className="input-icon">
              <Search size={14} strokeWidth={1.5} aria-hidden="true" />
              <input
                className="input"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder="Search memory"
                aria-label="Search memory pages"
              />
              {search ? (
                <button
                  type="button"
                  className="input-clear"
                  onClick={() => setSearch('')}
                  aria-label="Clear search"
                >
                  <X size={13} strokeWidth={1.5} />
                </button>
              ) : null}
            </div>
            <span className="memory-count mono" role="status">
              {loading ? 'searching' : `${pages.length} ${pages.length === 1 ? 'page' : 'pages'}`}
              {activeQuery ? ' matched' : ''}
            </span>
          </div>

          <div className="memory-list-scroll">
            {loading ? (
              <div className="memory-loading">
                <BlockSkeleton lines={6} />
              </div>
            ) : error ? (
              <div className="memory-loading">
                <ErrorState
                  error={error}
                  label="Memory could not be loaded."
                  onRetry={() => void active.refetch()}
                />
              </div>
            ) : pages.length === 0 ? (
              <EmptyState
                action={
                  activeQuery ? (
                    <button type="button" className="btn" onClick={() => setSearch('')}>
                      Clear search
                    </button>
                  ) : (
                    <Link to={`/assessments/${id}/runs`}>View runs</Link>
                  )
                }
              >
                {activeQuery ? 'No memory pages match this query.' : 'No memory pages yet.'}
              </EmptyState>
            ) : (
              <ul className="memory-items">
                {pages.map((page) => (
                  <li key={page.id}>
                    <button
                      type="button"
                      className={`memory-item${page.id === selectedId ? ' is-active' : ''}`}
                      onClick={() => setSelectedId(page.id)}
                      aria-current={page.id === selectedId ? 'true' : undefined}
                    >
                      <span className="memory-item-title">{page.title}</span>
                      <span className="memory-item-slug mono">{page.slug}</span>
                      <span className="memory-item-meta">
                        <span className="mono">v{page.version}</span>
                        <Status value={page.validity} />
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </aside>

        <article className="memory-doc" aria-label="Selected memory page">
          {!selected ? (
            <EmptyState>
              {loading ? 'Loading memory pages.' : 'Select a memory page to read it.'}
            </EmptyState>
          ) : (
            <>
              <header className="memory-doc-head">
                <div className="memory-doc-title">
                  <h2>{selected.title}</h2>
                  <span className="memory-doc-slug mono">{selected.slug}</span>
                </div>
                <div className="memory-doc-meta">
                  <span className="mono">v{selected.version}</span>
                  <Status value={selected.validity} />
                  <Status value={selected.state} />
                  <span className="mono quiet" title={formatDateTime(selected.created_at)}>
                    {selected.created_by} · {formatRelative(selected.created_at)}
                  </span>
                </div>
              </header>

              {selected.references.length > 0 ? (
                <div className="ref-list memory-refs">
                  {selected.references.map((reference, index) => (
                    <ReferenceChip
                      key={`${reference.kind}-${reference.id}-${index}`}
                      reference={reference}
                      assessmentId={id ?? ''}
                    />
                  ))}
                </div>
              ) : null}

              <div className="memory-body">
                {selected.body_markdown.trim() ? (
                  <SafeMarkdown markdown={selected.body_markdown} />
                ) : (
                  <p className="drawer-empty">This page has no content.</p>
                )}
              </div>
            </>
          )}
        </article>
      </div>

      {id ? (
        <ContextDrawer
          open={contextOpen}
          onClose={() => setContextOpen(false)}
          assessmentId={id}
          identities={identities.data ?? []}
        />
      ) : null}
    </>
  );
}
