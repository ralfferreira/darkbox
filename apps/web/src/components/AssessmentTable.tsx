import type { ReactNode } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import type { AssessmentSummary } from '../api/types';
import { formatNumber, formatPercent, formatRelative, truncate } from '../lib/format';
import { EmptyState } from './ui';

export function AssessmentTable({
  assessments,
  showCompany = true,
  emptyMessage = 'No assessments yet.',
  emptyAction,
}: {
  assessments: AssessmentSummary[];
  showCompany?: boolean;
  emptyMessage?: string;
  emptyAction?: ReactNode;
}) {
  const navigate = useNavigate();

  if (assessments.length === 0) {
    return <EmptyState action={emptyAction}>{emptyMessage}</EmptyState>;
  }

  return (
    <div className="table-wrap">
      <table className="data-table">
        <thead>
          <tr>
            <th scope="col">Assessment</th>
            {showCompany ? <th scope="col">Company</th> : null}
            <th scope="col">Environment</th>
            <th scope="col" className="num">
              Observed
            </th>
            <th scope="col" className="num">
              Known
            </th>
            <th scope="col" className="num">
              Coverage
            </th>
            <th scope="col" className="num">
              Open findings
            </th>
            <th scope="col">Last activity</th>
          </tr>
        </thead>
        <tbody>
          {assessments.map((assessment) => (
            <tr
              key={assessment.id}
              className="row-link"
              onClick={() => navigate(`/assessments/${assessment.id}`)}
            >
              <td>
                <Link
                  to={`/assessments/${assessment.id}`}
                  className="cell-primary"
                  onClick={(event) => event.stopPropagation()}
                >
                  {assessment.name}
                </Link>
                <span className="cell-sub">{truncate(assessment.objective, 86)}</span>
              </td>
              {showCompany ? (
                <td>
                  <Link
                    to={`/companies/${assessment.company_id}`}
                    className="subtle-link"
                    onClick={(event) => event.stopPropagation()}
                  >
                    {assessment.company_name}
                  </Link>
                </td>
              ) : null}
              <td>
                <span className="mono quiet">{assessment.environment}</span>
              </td>
              <td className="num mono">{formatNumber(assessment.observed_count)}</td>
              <td className="num mono quiet">{formatNumber(assessment.endpoint_count)}</td>
              <td className="num mono">
                {formatPercent(assessment.observed_count, assessment.endpoint_count)}
              </td>
              <td className="num mono">{formatNumber(assessment.open_findings)}</td>
              <td>
                <span title={assessment.last_activity_at}>
                  {formatRelative(assessment.last_activity_at)}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
