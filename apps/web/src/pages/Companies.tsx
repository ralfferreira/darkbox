import { useState } from 'react';
import { useCompanies } from '../api/queries';
import { CreateCompanyDialog } from '../components/CreateCompanyDialog';
import { useNavigate } from 'react-router-dom';
import { Plus } from 'lucide-react';
import {
  BlockSkeleton,
  EmptyState,
  ErrorState,
  PageHeader,
  Panel,
} from '../components/ui';
import { formatDateTime, formatNumber, formatRelative, truncate } from '../lib/format';

export function CompaniesPage() {
  const companies = useCompanies();
  const [dialogOpen, setDialogOpen] = useState(false);
  const navigate = useNavigate();
  const rows = companies.data ?? [];

  return (
    <>
      <PageHeader
        meta="workspace"
        title="Companies"
        description="Target organisations and the assessments grouped under them."
        actions={
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setDialogOpen(true)}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            Create company
          </button>
        }
      />
      <Panel flush>
        {companies.isLoading ? (
          <BlockSkeleton lines={6} />
        ) : companies.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={companies.error}
              label="Companies could not be loaded."
              onRetry={() => void companies.refetch()}
            />
          </div>
        ) : rows.length === 0 ? (
          <EmptyState
            action={
              <button type="button" className="btn" onClick={() => setDialogOpen(true)}>
                Create company
              </button>
            }
          >
            No companies yet.
          </EmptyState>
        ) : (
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th scope="col">Company</th>
                  <th scope="col" className="num">
                    Assessments
                  </th>
                  <th scope="col">Last activity</th>
                  <th scope="col">Created</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((company) => (
                  <tr
                    key={company.id}
                    className="row-link"
                    onClick={() => navigate(`/companies/${company.id}`)}
                  >
                    <td>
                      <span className="cell-primary">{company.name}</span>
                      <span className="cell-sub">
                        {company.description ? truncate(company.description, 96) : 'No description.'}
                      </span>
                    </td>
                    <td className="num mono">{formatNumber(company.assessment_count)}</td>
                    <td>
                      <span title={formatDateTime(company.last_activity_at)}>
                        {company.last_activity_at
                          ? formatRelative(company.last_activity_at)
                          : '—'}
                      </span>
                    </td>
                    <td className="mono quiet">{formatDateTime(company.created_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
      <CreateCompanyDialog open={dialogOpen} onClose={() => setDialogOpen(false)} />
    </>
  );
}
