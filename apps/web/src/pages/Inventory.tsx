import { useEffect, useState } from 'react';
import { useParams, useSearchParams } from 'react-router-dom';
import { Plus, Search } from 'lucide-react';
import { useIdentities, useInventory, type InventoryFilters } from '../api/queries';
import type { EndpointDto, IdentityDto } from '../api/types';
import { EndpointDrawer } from '../components/EndpointDrawer';
import { ImportUrlsDialog } from '../components/ImportUrlsDialog';
import { Status } from '../components/Status';
import {
  BlockSkeleton,
  EmptyState,
  ErrorState,
  PageHeader,
  TableLoading,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { formatNumber, formatRelative } from '../lib/format';

const PAGE_SIZE = 50;
const STATES = ['candidate', 'observed', 'unreachable', 'excluded'] as const;

interface FilterText {
  q: string;
  host: string;
  source: string;
}

export function InventoryPage() {
  const { id } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const session = useCurrentSession();
  const identities = useIdentities(id);
  const [importOpen, setImportOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const stateParam = searchParams.get('state') ?? '';
  const identityParam = searchParams.get('identity_id') ?? '';
  const focus = searchParams.get('focus');
  const offset = Math.max(0, Number(searchParams.get('offset') ?? 0) || 0);

  const [text, setText] = useState<FilterText>({
    q: searchParams.get('q') ?? '',
    host: searchParams.get('host') ?? '',
    source: searchParams.get('source') ?? '',
  });
  const debounced = useDebouncedValue(text, 300);

  useEffect(() => {
    const q = searchParams.get('q') ?? '';
    const host = searchParams.get('host') ?? '';
    const source = searchParams.get('source') ?? '';
    setText((current) =>
      current.q === q && current.host === host && current.source === source
        ? current
        : { q, host, source },
    );
  }, [searchParams]);

  useEffect(() => {
    const q = searchParams.get('q') ?? '';
    const host = searchParams.get('host') ?? '';
    const source = searchParams.get('source') ?? '';
    if (debounced.q === q && debounced.host === host && debounced.source === source) return;
    const next = new URLSearchParams(searchParams);
    const setOrDelete = (key: string, value: string) => {
      if (value) next.set(key, value);
      else next.delete(key);
    };
    setOrDelete('q', debounced.q.trim());
    setOrDelete('host', debounced.host.trim());
    setOrDelete('source', debounced.source.trim());
    next.delete('offset');
    setSearchParams(next, { replace: true });
    // Debounced text is the trigger; the URL is read at that moment.
  }, [debounced.q, debounced.host, debounced.source]);

  const filters: InventoryFilters = {
    state: stateParam || undefined,
    host: debounced.host.trim() || undefined,
    source: debounced.source.trim() || undefined,
    q: debounced.q.trim() || undefined,
    identity_id: identityParam || undefined,
    limit: PAGE_SIZE,
    offset,
  };

  const inventory = useInventory(id, filters);
  const endpoints = inventory.data?.endpoints ?? [];
  const counts = inventory.data?.counts ?? {};
  const total = inventory.data?.total ?? 0;
  const identityList = identities.data ?? [];

  useEffect(() => {
    if (!focus) return;
    if (endpoints.some((endpoint) => endpoint.id === focus)) {
      setSelectedId((current) => current ?? focus);
    }
  }, [focus, endpoints]);

  const updateParam = (key: string, value: string) => {
    const next = new URLSearchParams(searchParams);
    if (value) next.set(key, value);
    else next.delete(key);
    if (key !== 'offset') next.delete('offset');
    setSearchParams(next, { replace: true });
  };

  const resetFilters = () => {
    setText({ q: '', host: '', source: '' });
    setSearchParams(new URLSearchParams(), { replace: true });
  };

  const hasFilters = Boolean(
    stateParam || text.q || text.host || text.source || identityParam,
  );

  const selected = selectedId
    ? (endpoints.find((endpoint) => endpoint.id === selectedId) ?? null)
    : (focus ? (endpoints.find((endpoint) => endpoint.id === focus) ?? null) : null);

  const closeDrawer = () => {
    setSelectedId(null);
    if (focus) {
      const next = new URLSearchParams(searchParams);
      next.delete('focus');
      setSearchParams(next, { replace: true });
    }
  };

  const canWrite = session.data?.can_write ?? false;

  return (
    <>
      <PageHeader
        meta={`inventory${inventory.data ? ` · policy v${inventory.data.policy_version}` : ''}`}
        title="Inventory"
        description="Endpoints known within the effective scope."
        actions={
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setImportOpen(true)}
            disabled={!canWrite}
            title={canWrite ? undefined : 'Your role cannot start runs.'}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            Import URLs
          </button>
        }
      />

      {inventory.data ? (
        <div className="count-strip">
          {STATES.map((state) => (
            <span key={state} className="count-item">
              <Status value={state} />
              <span className="mono count-number">{formatNumber(counts[state] ?? 0)}</span>
            </span>
          ))}
          <span className="count-total mono">{formatNumber(total)} total</span>
        </div>
      ) : inventory.isLoading ? (
        <BlockSkeleton lines={1} />
      ) : null}

      <div className="filter-row" role="group" aria-label="Inventory filters">
        <div className="filter">
          <label htmlFor="filter-state" className="filter-label mono">
            State
          </label>
          <select
            id="filter-state"
            className="input select"
            value={stateParam}
            onChange={(event) => updateParam('state', event.target.value)}
          >
            <option value="">All states</option>
            {STATES.map((state) => (
              <option key={state} value={state}>
                {state}
              </option>
            ))}
          </select>
        </div>
        <div className="filter">
          <label htmlFor="filter-host" className="filter-label mono">
            Host
          </label>
          <input
            id="filter-host"
            className="input"
            value={text.host}
            onChange={(event) => setText({ ...text, host: event.target.value })}
            placeholder="host"
            spellCheck={false}
          />
        </div>
        <div className="filter">
          <label htmlFor="filter-source" className="filter-label mono">
            Source
          </label>
          <input
            id="filter-source"
            className="input"
            value={text.source}
            onChange={(event) => setText({ ...text, source: event.target.value })}
            placeholder="source"
            spellCheck={false}
          />
        </div>
        <div className="filter filter-search">
          <label htmlFor="filter-q" className="filter-label mono">
            Search
          </label>
          <div className="input-icon">
            <Search size={14} strokeWidth={1.5} aria-hidden="true" />
            <input
              id="filter-q"
              className="input"
              value={text.q}
              onChange={(event) => setText({ ...text, q: event.target.value })}
              placeholder="path, query, method"
            />
          </div>
        </div>
        <div className="filter">
          <label htmlFor="filter-identity" className="filter-label mono">
            Identity
          </label>
          <select
            id="filter-identity"
            className="input select"
            value={identityParam}
            onChange={(event) => updateParam('identity_id', event.target.value)}
          >
            <option value="">All identities</option>
            {identityList.map((identity) => (
              <option key={identity.id} value={identity.id}>
                {identity.label}
              </option>
            ))}
          </select>
        </div>
        <button
          type="button"
          className="btn btn-ghost filter-reset"
          onClick={resetFilters}
          disabled={!hasFilters}
        >
          Reset
        </button>
      </div>

      <div className="panel">
        {inventory.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={inventory.error}
              label="Inventory could not be loaded."
              onRetry={() => void inventory.refetch()}
            />
          </div>
        ) : (
          <>
            <div className="table-wrap">
              <table className="data-table inventory-table">
                <thead>
                  <tr>
                    <th scope="col">Method</th>
                    <th scope="col">Host</th>
                    <th scope="col">Path</th>
                    <th scope="col">State</th>
                    <th scope="col">Source</th>
                    <th scope="col">Identity</th>
                    <th scope="col">Recorded</th>
                  </tr>
                </thead>
                {inventory.isLoading && !inventory.data ? (
                  <TableLoading columns={7} rows={8} />
                ) : (
                  <tbody>
                    {endpoints.map((endpoint) => (
                      <tr
                        key={endpoint.id}
                        className="row-link"
                        onClick={() => setSelectedId(endpoint.id)}
                      >
                        <td className="mono method-cell">{endpoint.method}</td>
                        <td className="mono">{endpoint.host}</td>
                        <td>
                          <button
                            type="button"
                            className="row-open mono"
                            onClick={() => setSelectedId(endpoint.id)}
                            aria-label={`Open endpoint ${endpoint.method} ${endpoint.host}${endpoint.path}`}
                          >
                            {endpoint.path}
                          </button>
                          {endpoint.query_names.length > 0 ? (
                            <span className="cell-sub mono">
                              ?{endpoint.query_names.join(', ')}
                            </span>
                          ) : null}
                        </td>
                        <td>
                          <Status value={endpoint.state} />
                        </td>
                        <td className="mono quiet">{endpoint.source}</td>
                        <td>
                          {endpoint.identity_id ? (
                            <span className="quiet">
                              {identityList.find((item) => item.id === endpoint.identity_id)
                                ?.label ?? 'unknown'}
                            </span>
                          ) : (
                            <span className="muted">—</span>
                          )}
                        </td>
                        <td>
                          <span title={formatRelative(endpoint.recorded_at)}>
                            {formatRelative(endpoint.recorded_at)}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                )}
              </table>
            </div>

            {!inventory.isLoading && endpoints.length === 0 ? (
              <EmptyState
                action={
                  hasFilters ? (
                    <button type="button" className="btn" onClick={resetFilters}>
                      Reset filters
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="btn"
                      onClick={() => setImportOpen(true)}
                      disabled={!canWrite}
                    >
                      Import URLs
                    </button>
                  )
                }
              >
                {hasFilters
                  ? 'No endpoints match these filters.'
                  : 'No endpoints recorded yet. Import URLs or run discovery.'}
              </EmptyState>
            ) : null}

            {total > 0 ? (
              <div className="table-foot">
                <span className="mono quiet">
                  {formatNumber(offset + 1)}–{formatNumber(Math.min(offset + endpoints.length, total))}{' '}
                  of {formatNumber(total)}
                </span>
                <div className="table-foot-actions">
                  <button
                    type="button"
                    className="btn"
                    disabled={offset === 0}
                    onClick={() => updateParam('offset', String(Math.max(0, offset - PAGE_SIZE)))}
                  >
                    Previous
                  </button>
                  <button
                    type="button"
                    className="btn"
                    disabled={offset + PAGE_SIZE >= total}
                    onClick={() => updateParam('offset', String(offset + PAGE_SIZE))}
                  >
                    Next
                  </button>
                </div>
              </div>
            ) : null}
          </>
        )}
      </div>

      {id ? (
        <>
          <EndpointDrawer
            endpoint={selected}
            assessmentId={id}
            identities={identityList}
            onClose={closeDrawer}
          />
          {focus && selectedId === null && !selected ? (
            <FocusedEndpoint
              assessmentId={id}
              focus={focus}
              identities={identityList}
              onClose={closeDrawer}
            />
          ) : null}
          <ImportUrlsDialog
            open={importOpen}
            onClose={() => setImportOpen(false)}
            assessmentId={id}
          />
        </>
      ) : null}
    </>
  );
}

function FocusedEndpoint({
  assessmentId,
  focus,
  identities,
  onClose,
}: {
  assessmentId: string;
  focus: string;
  identities: IdentityDto[];
  onClose: () => void;
}) {
  const inventory = useInventory(assessmentId, { limit: 200 });
  const endpoint: EndpointDto | undefined = (inventory.data?.endpoints ?? []).find(
    (item) => item.id === focus,
  );

  if (!endpoint) return null;

  return (
    <EndpointDrawer
      endpoint={endpoint}
      assessmentId={assessmentId}
      identities={identities}
      onClose={onClose}
    />
  );
}
