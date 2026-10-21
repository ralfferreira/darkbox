import { useState } from 'react';
import { Plus } from 'lucide-react';
import { useAllAssessments } from '../api/queries';
import { AssessmentTable } from '../components/AssessmentTable';
import { CreateAssessmentDialog } from '../components/CreateAssessmentDialog';
import { BlockSkeleton, ErrorState, PageHeader, Panel } from '../components/ui';

export function AssessmentsPage() {
  const assessments = useAllAssessments();
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <>
      <PageHeader
        meta="workspace"
        title="Assessments"
        description="Every assessment in this workspace, most recently active first."
        actions={
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setDialogOpen(true)}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            New assessment
          </button>
        }
      />
      <Panel flush>
        {assessments.isLoading ? (
          <BlockSkeleton lines={8} />
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
            assessments={assessments.data ?? []}
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
      <CreateAssessmentDialog open={dialogOpen} onClose={() => setDialogOpen(false)} />
    </>
  );
}
