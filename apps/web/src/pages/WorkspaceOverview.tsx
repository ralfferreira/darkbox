import { useState } from 'react';
import { Link } from 'react-router-dom';
import { Plus } from 'lucide-react';
import { useAllAssessments, useCapabilities, useCompanies } from '../api/queries';
import { AssessmentTable } from '../components/AssessmentTable';
import { CreateAssessmentDialog } from '../components/CreateAssessmentDialog';
import { CreateCompanyDialog } from '../components/CreateCompanyDialog';
import { Status } from '../components/Status';
import {
  BlockSkeleton,
  EmptyState,
  ErrorState,
  Metric,
  PageHeader,
  Panel,
} from '../components/ui';
import { formatNumber, formatPercent } from '../lib/format';

export function WorkspaceOverviewPage() {
  const assessments = useAllAssessments();
  const companies = useCompanies();
  const capabilities = useCapabilities();
  const [companyDialog, setCompanyDialog] = useState(false);
  const [assessmentDialog, setAssessmentDialog] = useState(false);

  const rows = assessments.data ?? [];
  const adapters = capabilities.data?.adapters ?? [];
  const totalKnown = rows.reduce((sum, item) => sum + item.endpoint_count, 0);
  const totalObserved = rows.reduce((sum, item) => sum + item.observed_count, 0);
  const totalFindings = rows.reduce((sum, item) => sum + item.open_findings, 0);

  return (
    <>
      <PageHeader
        meta="workspace overview"
        title="Overview"
        description="Assessment coverage and adapter availability for this workspace."
        actions={
          <>
            <button type="button" className="btn" onClick={() => setCompanyDialog(true)}>
              Create company
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => setAssessmentDialog(true)}
            >
              <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
              New assessment
            </button>
          </>
        }
      />

      <div className="metrics">
        <Metric label="Assessments" value={formatNumber(assessments.data?.length)} />
        <Metric label="Companies" value={formatNumber(companies.data?.length)} />
        <Metric
          label="Observed / known"
          value={
            <span className="mono">
              {formatNumber(totalObserved)}
              <span className="metric-fraction"> / {formatNumber(totalKnown)}</span>
            </span>
          }
          hint={formatPercent(totalObserved, totalKnown)}
        />
        <Metric label="Open findings" value={formatNumber(totalFindings)} />
      </div>

      <Panel
        title="Capabilities"
        subtitle="Adapters available to this API instance."
        actions={<Link to="/capabilities">All capabilities</Link>}
      >
        {capabilities.isLoading ? (
          <BlockSkeleton lines={2} />
        ) : capabilities.isError ? (
          <ErrorState
            error={capabilities.error}
            label="Capabilities could not be loaded."
            onRetry={() => void capabilities.refetch()}
          />
        ) : adapters.length === 0 ? (
          <EmptyState>No adapters are registered.</EmptyState>
        ) : (
          <ul className="cap-strip">
            {adapters.map((adapter) => (
              <li key={adapter.id} className="cap-item">
                <div className="cap-item-head">
                  <Status value={adapter.state} />
                  <span className="mono cap-id">{adapter.id}</span>
                </div>
                <span className="cap-meta">
                  {adapter.provider} <span className="mono">v{adapter.version}</span>
                </span>
                {adapter.detail ? <span className="cap-detail">{adapter.detail}</span> : null}
              </li>
            ))}
          </ul>
        )}
      </Panel>

      <Panel
        title="Assessments"
        subtitle={
          assessments.data
            ? `${rows.length} across ${companies.data?.length ?? 0} companies`
            : undefined
        }
        flush
      >
        {assessments.isLoading ? (
          <BlockSkeleton lines={6} />
        ) : assessments.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={assessments.error}
              label="Assessments could not be loaded."
              onRetry={() => void assessments.refetch()}
            />
          </div>
        ) : (
          <AssessmentTable
            assessments={rows}
            emptyAction={
              <button
                type="button"
                className="btn"
                onClick={() => setAssessmentDialog(true)}
              >
                New assessment
              </button>
            }
          />
        )}
      </Panel>

      <CreateCompanyDialog open={companyDialog} onClose={() => setCompanyDialog(false)} />
      <CreateAssessmentDialog
        open={assessmentDialog}
        onClose={() => setAssessmentDialog(false)}
      />
    </>
  );
}
