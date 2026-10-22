import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { requestBlob } from '../api/client';
import type { EvidenceDto } from '../api/types';
import { useEvidenceContent } from '../api/queries';
import { formatBytes, formatDateTime, shortHash, shortId } from '../lib/format';
import { Drawer } from './Drawer';
import { Status } from './Status';
import { BlockSkeleton, CopyButton, ErrorState, IdChip, KeyValue } from './ui';
import { useToast } from './Toast';

export function EvidenceDrawer({
  evidence,
  assessmentId,
  onClose,
}: {
  evidence: EvidenceDto | null;
  assessmentId: string;
  onClose: () => void;
}) {
  const isImage = Boolean(evidence?.content_type.startsWith('image/'));
  const content = useEvidenceContent(evidence && !isImage ? evidence.id : null);
  const image = useQuery({
    queryKey: ['evidence-image-drawer', evidence?.id],
    queryFn: () => requestBlob(`/evidence/${evidence!.id}/content`),
    enabled: Boolean(evidence && isImage),
    staleTime: Infinity,
    retry: false,
  });
  const { notify } = useToast();
  const [downloading, setDownloading] = useState(false);
  const [imageUrl, setImageUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!image.data) {
      setImageUrl(null);
      return;
    }
    const next = URL.createObjectURL(image.data);
    setImageUrl(next);
    return () => URL.revokeObjectURL(next);
  }, [image.data]);

  if (!evidence) return null;

  const download = () => {
    if (!content.data && !image.data) return;
    setDownloading(true);
    try {
      const blob = isImage
        ? image.data!
        : new Blob([content.data ?? ''], { type: 'text/plain;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = `darkbox-evidence-${shortId(evidence.id)}.${isImage ? 'jpg' : 'txt'}`;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
    } catch {
      notify('Could not prepare the download.', 'error');
    } finally {
      setDownloading(false);
    }
  };

  return (
    <Drawer
      open
      onClose={onClose}
      title={evidence.kind.replace(/_/g, ' ')}
      subtitle={
        <span className="drawer-sub-line">
          <Status value={evidence.classification} />
          <span className="mono">{formatBytes(evidence.size_bytes)}</span>
          <span className="mono muted">{shortHash(evidence.sha256)}</span>
        </span>
      }
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Close
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={download}
            disabled={(!content.data && !image.data) || downloading}
          >
            {downloading ? 'Preparing…' : isImage ? 'Download image' : 'Download text'}
          </button>
        </>
      }
    >
      <section className="drawer-section">
        <div className="evidence-badges">
          {evidence.sanitized ? (
            <span className="badge" data-tone="lime">
              sanitized
            </span>
          ) : (
            <span className="badge" data-tone="amber">
              not sanitized
            </span>
          )}
          {evidence.truncated ? (
            <span className="badge" data-tone="amber">
              truncated at capture
            </span>
          ) : null}
          <span className="badge" data-tone="neutral">
            {isImage ? 'image evidence' : 'text evidence'}
          </span>
        </div>
        <p className="drawer-note">{evidence.summary}</p>
        <KeyValue
          items={[
            {
              label: 'Classification',
              value: <Status value={evidence.classification} />,
            },
            { label: 'Content type', value: <span className="mono">{evidence.content_type}</span> },
            { label: 'Size', value: <span className="mono">{formatBytes(evidence.size_bytes)}</span> },
            {
              label: 'SHA-256',
              value: (
                <span className="hash-row">
                  <span className="mono break-any">{evidence.sha256}</span>
                  <CopyButton value={evidence.sha256} />
                </span>
              ),
            },
            {
              label: 'Run',
              value: evidence.run_id ? (
                <Link to={`/assessments/${assessmentId}/runs/${evidence.run_id}`} className="mono">
                  {shortId(evidence.run_id)}
                </Link>
              ) : (
                <span className="muted">none</span>
              ),
            },
            {
              label: 'Step',
              value: evidence.step_id ? (
                <span className="mono">{shortId(evidence.step_id)}</span>
              ) : (
                <span className="muted">none</span>
              ),
            },
            {
              label: 'Identity',
              value: evidence.identity_id ? (
                <span className="mono">{shortId(evidence.identity_id)}</span>
              ) : (
                <span className="muted">none</span>
              ),
            },
            { label: 'Created', value: formatDateTime(evidence.created_at) },
            {
              label: 'Retention',
              value: evidence.retention_until
                ? formatDateTime(evidence.retention_until)
                : <span className="muted">none</span>,
            },
            { label: 'ID', value: <IdChip id={evidence.id} length={12} /> },
          ]}
        />
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">Content</h3>
        {evidence.truncated ? (
          <p className="notice" data-tone="amber">
            The capture was truncated at the configured artifact limit. The text below is the
            stored portion only.
          </p>
        ) : null}
        {isImage ? (
          image.isLoading ? (
            <BlockSkeleton lines={6} />
          ) : image.isError || !imageUrl ? (
            <ErrorState
              error={image.error}
              label="Evidence image could not be loaded."
              onRetry={() => void image.refetch()}
            />
          ) : (
            <figure className="evidence-image">
              <img src={imageUrl} alt={evidence.summary} />
            </figure>
          )
        ) : content.isLoading ? (
          <BlockSkeleton lines={8} />
        ) : content.isError ? (
          <ErrorState
            error={content.error}
            label="Evidence content could not be loaded."
            onRetry={() => void content.refetch()}
          />
        ) : (
          <pre className="evidence-content">{content.data}</pre>
        )}
      </section>
    </Drawer>
  );
}
