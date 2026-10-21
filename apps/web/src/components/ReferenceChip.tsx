import { Link } from 'react-router-dom';
import type { MemoryReferenceDto } from '../api/types';

function referencePath(assessmentId: string, reference: MemoryReferenceDto): string | null {
  const id = encodeURIComponent(reference.id);
  switch (reference.kind) {
    case 'evidence':
      return `/assessments/${assessmentId}/evidence?focus=${id}`;
    case 'endpoint':
      return `/assessments/${assessmentId}/inventory?focus=${id}`;
    case 'flow':
      return `/assessments/${assessmentId}/flows?focus=${id}`;
    case 'finding':
      return `/assessments/${assessmentId}/findings?focus=${id}`;
    case 'run':
      return `/assessments/${assessmentId}/runs/${id}`;
    default:
      return null;
  }
}

export function ReferenceChip({
  reference,
  assessmentId,
}: {
  reference: MemoryReferenceDto;
  assessmentId: string;
}) {
  const path = referencePath(assessmentId, reference);
  const content = (
    <>
      <span className="ref-kind mono">{reference.kind}</span>
      <span>{reference.label}</span>
    </>
  );
  if (!path) {
    return (
      <span className="ref-chip" title={`${reference.kind} ${reference.id}`}>
        {content}
      </span>
    );
  }
  return (
    <Link className="ref-chip" to={path} title={`${reference.kind} ${reference.id}`}>
      {content}
    </Link>
  );
}
