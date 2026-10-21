import { useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Search, CornerDownLeft } from 'lucide-react';
import { useAllAssessments, useCompanies } from '../api/queries';
import { useDialogFocus } from '../hooks/useDialogFocus';
import { commandKeyLabel } from '../lib/platform';

interface PaletteItem {
  id: string;
  label: string;
  hint: string;
  path: string;
  kind: 'navigation' | 'assessment' | 'company';
}

const BASE_ITEMS: PaletteItem[] = [
  { id: 'nav-overview', label: 'Workspace overview', hint: 'workspace', path: '/', kind: 'navigation' },
  { id: 'nav-companies', label: 'Companies', hint: 'workspace', path: '/companies', kind: 'navigation' },
  { id: 'nav-assessments', label: 'Assessments', hint: 'workspace', path: '/assessments', kind: 'navigation' },
  { id: 'nav-capabilities', label: 'Capabilities', hint: 'workspace', path: '/capabilities', kind: 'navigation' },
];

function fuzzyScore(query: string, text: string): number | null {
  if (!query) return 1;
  const needle = query.toLowerCase();
  const haystack = text.toLowerCase();
  if (haystack.startsWith(needle)) return 120 - haystack.length;
  const index = haystack.indexOf(needle);
  if (index >= 0) return 90 - index;

  let position = 0;
  let score = 20;
  let streak = 0;
  for (const char of needle) {
    const found = haystack.indexOf(char, position);
    if (found === -1) return null;
    streak = found === position ? streak + 1 : 0;
    score += streak > 1 ? 3 : 1;
    position = found + 1;
  }
  return score;
}

export function CommandPalette({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const navigate = useNavigate();
  const panelRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const assessments = useAllAssessments();
  const companies = useCompanies();
  useDialogFocus(open, onClose, panelRef);

  useEffect(() => {
    if (open) {
      setQuery('');
      setActive(0);
      window.requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const items = useMemo(() => {
    const assessmentItems: PaletteItem[] = (assessments.data ?? []).map((assessment) => ({
      id: `assessment-${assessment.id}`,
      label: assessment.name,
      hint: `${assessment.company_name} · ${assessment.environment}`,
      path: `/assessments/${assessment.id}`,
      kind: 'assessment',
    }));
    const companyItems: PaletteItem[] = (companies.data ?? []).map((company) => ({
      id: `company-${company.id}`,
      label: company.name,
      hint: 'company',
      path: `/companies/${company.id}`,
      kind: 'company',
    }));
    const all = [...BASE_ITEMS, ...companyItems, ...assessmentItems];
    const trimmed = query.trim();
    if (!trimmed) return all.slice(0, 14);
    return all
      .map((item) => ({
        item,
        score: fuzzyScore(trimmed, `${item.label} ${item.hint}`),
      }))
      .filter((entry): entry is { item: PaletteItem; score: number } => entry.score !== null)
      .sort((a, b) => b.score - a.score)
      .slice(0, 14)
      .map((entry) => entry.item);
  }, [assessments.data, companies.data, query]);

  useEffect(() => {
    setActive((current) => Math.min(current, Math.max(0, items.length - 1)));
  }, [items.length]);

  const run = (item: PaletteItem | undefined) => {
    if (!item) return;
    onClose();
    navigate(item.path);
  };

  if (!open) return null;

  return (
    <div
      className="overlay palette-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panelRef}
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        tabIndex={-1}
      >
        <div className="palette-search">
          <Search size={16} strokeWidth={1.5} aria-hidden="true" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
            onKeyDown={(event) => {
              if (event.key === 'ArrowDown') {
                event.preventDefault();
                setActive((current) => (items.length ? (current + 1) % items.length : 0));
              } else if (event.key === 'ArrowUp') {
                event.preventDefault();
                setActive((current) =>
                  items.length ? (current - 1 + items.length) % items.length : 0,
                );
              } else if (event.key === 'Enter') {
                event.preventDefault();
                run(items[active]);
              }
            }}
            placeholder="Jump to an assessment or section"
            aria-label="Search commands"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-results"
            aria-activedescendant={items[active] ? `palette-${active}` : undefined}
            autoComplete="off"
            spellCheck={false}
          />
          <kbd>{commandKeyLabel}</kbd>
        </div>
        <ul className="palette-results" id="palette-results" role="listbox" aria-label="Commands">
          {items.length === 0 ? (
            <li className="palette-empty">
              No match. {assessments.isError ? 'Assessment list is unavailable.' : null}
            </li>
          ) : (
            items.map((item, index) => (
              <li
                key={item.id}
                id={`palette-${index}`}
                role="option"
                aria-selected={index === active}
                className={`palette-item${index === active ? ' is-active' : ''}`}
                onMouseEnter={() => setActive(index)}
                onClick={() => run(item)}
              >
                <span className="palette-item-main">
                  <span className="palette-label">{item.label}</span>
                  <span className="palette-hint mono">{item.hint}</span>
                </span>
                <span className="palette-kind mono">{item.kind}</span>
                {index === active ? (
                  <CornerDownLeft size={13} strokeWidth={1.5} aria-hidden="true" />
                ) : null}
              </li>
            ))
          )}
        </ul>
        <footer className="palette-foot mono">
          <span>↑↓ navigate</span>
          <span>↵ open</span>
          <span>esc close</span>
        </footer>
      </div>
    </div>
  );
}
