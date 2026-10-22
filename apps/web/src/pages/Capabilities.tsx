import { useBrowserCapability, useCapabilities } from '../api/queries';
import { Status } from '../components/Status';
import {
  EmptyState,
  ErrorState,
  KeyValue,
  PageHeader,
  Panel,
  TableLoading,
} from '../components/ui';

export function CapabilitiesPage() {
  const capabilities = useCapabilities();
  const browser = useBrowserCapability();

  return (
    <>
      <PageHeader
        meta="workspace"
        title="Capabilities"
        description="Adapter and provider availability reported by this instance."
      />

      {capabilities.isError ? (
        <ErrorState
          error={capabilities.error}
          label="Capabilities could not be loaded."
          onRetry={() => void capabilities.refetch()}
        />
      ) : (
        <>
          <div className="panel">
            <div className="table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    <th scope="col">Adapter</th>
                    <th scope="col">State</th>
                    <th scope="col">Provider</th>
                    <th scope="col">Version</th>
                    <th scope="col">Detail</th>
                  </tr>
                </thead>
                {capabilities.isLoading ? (
                  <TableLoading columns={5} rows={5} />
                ) : (
                  <tbody>
                    {(capabilities.data?.adapters ?? []).map((adapter) => (
                      <tr key={adapter.id}>
                        <td className="mono">{adapter.id}</td>
                        <td>
                          <Status value={adapter.state} />
                        </td>
                        <td>{adapter.provider}</td>
                        <td className="mono">{adapter.version}</td>
                        <td className="quiet">{adapter.detail || '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                )}
              </table>
            </div>
            {!capabilities.isLoading &&
            (capabilities.data?.adapters.length ?? 0) === 0 ? (
              <EmptyState>No adapters are registered.</EmptyState>
            ) : null}
          </div>

          <Panel
            title="Functional browser probe"
            subtitle="A real engine starts, executes JavaScript and reports its own name and version."
            actions={
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => void browser.refetch()}
              >
                Run probe
              </button>
            }
          >
            {browser.isLoading ? (
              <p className="mono quiet">starting the browser host and executing a JavaScript probe…</p>
            ) : browser.isError ? (
              <ErrorState
                error={browser.error}
                label="The browser probe did not pass."
                onRetry={() => void browser.refetch()}
              />
            ) : browser.data ? (
              <KeyValue
                items={[
                  { label: 'State', value: <Status value={browser.data.state} /> },
                  { label: 'Engine', value: <span className="mono">{browser.data.provider}</span> },
                  { label: 'Version', value: <span className="mono">{browser.data.version}</span> },
                  { label: 'Detail', value: browser.data.detail },
                ]}
              />
            ) : null}
          </Panel>

          {capabilities.data ? (
            <Panel
              title="Provider"
              subtitle="Model and lab configuration reported by the API."
            >
              <KeyValue
                items={[
                  {
                    label: 'Model provider',
                    value: <span className="mono">{capabilities.data.model_provider || 'none'}</span>,
                  },
                  {
                    label: 'Lab URL',
                    value: capabilities.data.lab_url ? (
                      <a
                        href={capabilities.data.lab_url}
                        target="_blank"
                        rel="noreferrer noopener"
                        className="mono break-any"
                      >
                        {capabilities.data.lab_url}
                      </a>
                    ) : (
                      <span className="muted">none</span>
                    ),
                  },
                  {
                    label: 'Raw evidence',
                    value: (
                      <Status
                        value={capabilities.data.raw_evidence_enabled ? 'available' : 'unavailable'}
                        label={capabilities.data.raw_evidence_enabled ? 'enabled' : 'disabled'}
                        tone={capabilities.data.raw_evidence_enabled ? 'lime' : 'neutral'}
                      />
                    ),
                  },
                ]}
              />
            </Panel>
          ) : null}
        </>
      )}
    </>
  );
}
