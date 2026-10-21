import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { Plus } from 'lucide-react';
import { useAssessments, useCompany } from '../api/queries';
import { AssessmentTable } from '../components/AssessmentTable';
import { CreateAssessmentDialog } from '../components/CreateAssessmentDialog';
import {
  BlockSkeleton,
  ErrorState,
  Metric,
  PageHeader,
  Panel,
} from '../components/ui';
import { formatDateTime, formatNumber, formatPercent, formatRelative } from '../lib/format';

export function CompanyDetailPage() {
  const { id } = useParams<{ id: string }>();
  const company = useCompany(id);
  const assessments = useAssessments(company.data?.id);
  const [dialogOpen, setDialogOpen] = useState(false);

  const rows = assessments.data ?? [];
  const known = rows.reduce((sum, item) => sum + item.endpoint_count, 0);
  const observed = rows.reduce((sum, item) => sum + item.observed_count, 0);
  const findings = rows.reduce((sum, item) => sum + item.open_findings, 0);

  if (company.isError) {
    return (
      <ErrorState
        error={company.error}
        label="Company could not be loaded."
        onRetry={() => void company.refetch()}
      />
    );
  }

  return (
    <>
      <PageHeader
        meta={
          company.data
            ? `company · created ${formatDateTime(company.data.created_at)}`
            : 'company'
        }
        title={company.data?.name ?? 'Company'}
        description={company.data?.description || 'No description recorded.'}
        actions={
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setDialogOpen(true)}
            disabled={!company.data}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            New assessment
          </button>
        }
      />

      <div className="metrics">
        <Metric label="Assessments" value={formatNumber(company.data?.assessment_count)} />
        <Metric
          label="Observed / known"
          value={
            <span className="mono">
              {formatNumber(observed)}
              <span className="metric-fraction"> / {formatNumber(known)}</span>
            </span>
          }
          hint={formatPercent(observed, known)}
        />
        <Metric label="Open findings" value={formatNumber(findings)} />
        <Metric
          label="Last activity"
          value={
            company.data?.last_activity_at
              ? formatRelative(company.data.last_activity_at)
              : '—'
          }
        />
      </div>

      <Panel title="Assessments" flush>
        {company.isLoading || assessments.isLoading ? (
          <BlockSkeleton lines={5} />
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
            showCompany={false}
            emptyMessage="No assessments for this company yet."
            emptyAction={
              <button
                type="button"
                className="btn"
                onClick={() => setDialogOpen(true)}
              >
                New assessment
              </button>
            }
          />
        )}
      </Panel>

      {id ? (
        <CreateAssessmentDialog
          open={dialogOpen}
          onClose={() => setDialogOpen(false)}
          companyId={id}
        />
      ) : null}
    </>
  );
}
