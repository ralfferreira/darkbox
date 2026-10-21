import { useEffect, useState } from 'react';
import { Link, Outlet, useLocation, useNavigate } from 'react-router-dom';
import {
  Boxes,
  Brain,
  Building2,
  ChevronDown,
  Database,
  FileText,
  Flag,
  GitBranch,
  LayoutDashboard,
  ListChecks,
  LogOut,
  Menu,
  Play,
  ScanSearch,
  Search,
  ShieldCheck,
  X,
  type LucideIcon,
} from 'lucide-react';
import { useAssessment, useAllAssessments, useCompany, useSession } from '../api/queries';
import { isTerminalRun } from '../api/types';
import { useAuth } from './AuthContext';
import { LiveDot } from './Status';
import { CommandPalette } from './CommandPalette';
import { ShortcutsDialog } from './ShortcutsDialog';
import { commandKeyLabel } from '../lib/platform';

interface NavItem {
  key: string;
  label: string;
  path: string;
  icon: LucideIcon;
}

const WORKSPACE_NAV: NavItem[] = [
  { key: 'overview', label: 'Overview', path: '/', icon: LayoutDashboard },
  { key: 'companies', label: 'Companies', path: '/companies', icon: Building2 },
  { key: 'assessments', label: 'Assessments', path: '/assessments', icon: Boxes },
];

const ASSESSMENT_NAV: NavItem[] = [
  { key: 'overview', label: 'Overview', path: '', icon: LayoutDashboard },
  { key: 'inventory', label: 'Inventory', path: '/inventory', icon: Database },
  { key: 'flows', label: 'Flows', path: '/flows', icon: GitBranch },
  { key: 'runs', label: 'Runs', path: '/runs', icon: Play },
  { key: 'evidence', label: 'Evidence', path: '/evidence', icon: FileText },
  { key: 'memory', label: 'Memory', path: '/memory', icon: Brain },
  { key: 'findings', label: 'Findings', path: '/findings', icon: Flag },
  { key: 'scope', label: 'Scope', path: '/scope', icon: ScanSearch },
  { key: 'procedures', label: 'Procedures', path: '/procedures', icon: ListChecks },
];

const ASSESSMENT_SECTIONS = new Set(ASSESSMENT_NAV.map((item) => item.key));

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return Boolean(target.closest('input, textarea, select, [contenteditable="true"]'));
}

export function AppShell() {  const { pathname } = useLocation();
  const navigate = useNavigate();
  const { token, signOut } = useAuth();
  const session = useSession(token);
  const [mobileOpen, setMobileOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);

  const assessmentMatch = pathname.match(/^\/assessments\/([^/]+)/);
  const assessmentId = assessmentMatch?.[1];
  const companyMatch = pathname.match(/^\/companies\/([^/]+)$/);
  const companyRouteId = companyMatch?.[1];

  const assessment = useAssessment(assessmentId);
  const company = useCompany(companyRouteId);
  const assessments = useAllAssessments();

  const suffix = assessmentId
    ? pathname.slice(`/assessments/${assessmentId}`.length)
    : '';
  const section = suffix.split('/')[1] ?? '';
  const activeRun = assessment.data?.recent_runs.find((run) => !isTerminalRun(run.state));

  useEffect(() => {
    setMobileOpen(false);
  }, [pathname]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setPaletteOpen((open) => !open);
        return;
      }
      if (event.key === '?' && !isTypingTarget(event.target)) {
        event.preventDefault();
        setShortcutsOpen(true);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  const switchAssessment = (nextId: string) => {
    let nextSuffix = '';
    if (section === 'runs') nextSuffix = '/runs';
    else if (ASSESSMENT_SECTIONS.has(section)) nextSuffix = `/${section}`;
    navigate(`/assessments/${nextId}${nextSuffix}`);
  };

  const breadcrumbs: Array<{ label: string; to?: string }> = [
    { label: session.data?.workspace_name ?? 'Workspace', to: '/' },
  ];

  if (assessmentId) {
    const assessmentCompany = assessment.data?.assessment;
    breadcrumbs.push({
      label: assessmentCompany?.company_name ?? 'Company',
      to: assessmentCompany ? `/companies/${assessmentCompany.company_id}` : undefined,
    });
    breadcrumbs.push({
      label: assessmentCompany?.name ?? 'Assessment',
    });
  } else if (companyRouteId) {
    breadcrumbs.push({ label: 'Companies', to: '/companies' });
    breadcrumbs.push({ label: company.data?.name ?? 'Company' });
  } else if (pathname.startsWith('/companies')) {
    breadcrumbs.push({ label: 'Companies' });
  } else if (pathname === '/assessments') {
    breadcrumbs.push({ label: 'Assessments' });
  } else if (pathname === '/capabilities') {
    breadcrumbs.push({ label: 'Capabilities' });
  }

  const assessmentOptions = assessments.data ?? [];

  return (
    <div className="app">
      <a className="skip-link" href="#main">
        Skip to content
      </a>

      <button
        type="button"
        className={`sidebar-scrim${mobileOpen ? ' is-open' : ''}`}
        aria-label="Close navigation"
        onClick={() => setMobileOpen(false)}
      />

      <aside id="sidebar" className={`sidebar${mobileOpen ? ' is-open' : ''}`}>
        <div className="brand-row">
          <Link to="/" className="brand" aria-label="DarkBox workspace overview">
            <span className="brand-glyph" aria-hidden="true">
              <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
                <rect
                  x="2.25"
                  y="2.25"
                  width="11.5"
                  height="11.5"
                  rx="0.5"
                  stroke="currentColor"
                  strokeWidth="1.5"
                />
                <path d="M5.5 8h5M8 5.5v5" stroke="currentColor" strokeWidth="1.5" />
              </svg>
            </span>
            <span className="brand-word mono">DARKBOX</span>
          </Link>
          <button
            type="button"
            className="btn btn-icon btn-ghost sidebar-close"
            aria-label="Close navigation"
            onClick={() => setMobileOpen(false)}
          >
            <X size={16} strokeWidth={1.5} />
          </button>
        </div>

        <div className="workspace-block">
          <span className="side-label mono">Workspace</span>
          <span className="workspace-name">{session.data?.workspace_name ?? '—'}</span>
        </div>

        <div className="sidebar-scroll">
          <nav aria-label="Workspace">
            <ul className="nav-list">
              {WORKSPACE_NAV.map((item) => {
                const active =
                  item.path === '/'
                    ? pathname === '/'
                    : item.path === '/companies'
                      ? pathname.startsWith('/companies')
                      : pathname === '/assessments';
                const Icon = item.icon;
                return (
                  <li key={item.key}>
                    <Link
                      to={item.path}
                      className={`nav-item${active ? ' is-active' : ''}`}
                      aria-current={active ? 'page' : undefined}
                    >
                      <Icon size={15} strokeWidth={1.5} aria-hidden="true" />
                      <span>{item.label}</span>
                    </Link>
                  </li>
                );
              })}
            </ul>
          </nav>

          {assessmentId ? (
            <div className="assessment-block">
              <div className="side-label-row">
                <span className="side-label mono">Assessment</span>
              </div>
              <div className="assessment-switch">
                <select
                  aria-label="Switch assessment"
                  value={assessmentId}
                  onChange={(event) => switchAssessment(event.target.value)}
                >
                  {assessmentId && !assessmentOptions.some((item) => item.id === assessmentId) ? (
                    <option value={assessmentId}>Current assessment</option>
                  ) : null}
                  {assessmentOptions.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name}
                    </option>
                  ))}
                </select>
                <ChevronDown size={13} strokeWidth={1.5} aria-hidden="true" />
              </div>
              <nav aria-label="Assessment sections">
                <ul className="nav-list">
                  {ASSESSMENT_NAV.map((item) => {
                    const active =
                      item.key === 'overview' ? section === '' : section === item.key;
                    const Icon = item.icon;
                    return (
                      <li key={item.key}>
                        <Link
                          to={`/assessments/${assessmentId}${item.path}`}
                          className={`nav-item${active ? ' is-active' : ''}`}
                          aria-current={active ? 'page' : undefined}
                        >
                          <Icon size={15} strokeWidth={1.5} aria-hidden="true" />
                          <span>{item.label}</span>
                        </Link>
                      </li>
                    );
                  })}
                </ul>
              </nav>
            </div>
          ) : null}
        </div>

        <footer className="side-foot">
          <div className="side-operator">
            <span className="operator-name">{session.data?.operator ?? '—'}</span>
            <span className="operator-role mono">
              {session.data?.role ?? '—'}
              {session.data?.can_write === false ? ' · read only' : ''}
            </span>
          </div>
          <div className="side-foot-actions">
            <Link to="/capabilities" className="foot-link">
              <ShieldCheck size={14} strokeWidth={1.5} aria-hidden="true" />
              Capabilities
            </Link>
            <button type="button" className="foot-link" onClick={signOut}>
              <LogOut size={14} strokeWidth={1.5} aria-hidden="true" />
              Sign out
            </button>
          </div>
        </footer>
      </aside>

      <div className="shell-main">
        <header className="topbar">
          <button
            type="button"
            className="btn btn-icon btn-ghost mobile-menu"
            aria-label="Open navigation"
            aria-expanded={mobileOpen}
            aria-controls="sidebar"
            onClick={() => setMobileOpen(true)}
          >
            <Menu size={17} strokeWidth={1.5} />
          </button>

          <nav className="breadcrumbs" aria-label="Breadcrumb">
            <ol>
              {breadcrumbs.map((crumb, index) => {
                const current = index === breadcrumbs.length - 1;
                return (
                  <li key={`${crumb.label}-${index}`}>
                    {index > 0 ? <span className="crumb-sep">/</span> : null}
                    {crumb.to && !current ? (
                      <Link to={crumb.to}>{crumb.label}</Link>
                    ) : (
                      <span aria-current={current ? 'page' : undefined}>{crumb.label}</span>
                    )}
                  </li>
                );
              })}
            </ol>
          </nav>

          <div className="topbar-right">
            {assessment.data ? (
              <span className="env-pill mono">{assessment.data.assessment.environment}</span>
            ) : null}
            {activeRun ? (
              <span
                className="live-indicator mono"
                title="A run is active; its event stream is live"
              >
                <LiveDot />
                <span>live</span>
              </span>
            ) : null}
            <button
              type="button"
              className="command-trigger"
              onClick={() => setPaletteOpen(true)}
              aria-label={`Command ${commandKeyLabel}`}
            >
              <Search size={14} strokeWidth={1.5} aria-hidden="true" />
              <span className="command-trigger-label">Command</span>
              <kbd>{commandKeyLabel}</kbd>
            </button>
          </div>
        </header>

        <main id="main" className="content" tabIndex={-1}>
          <div className="page">
            <Outlet />
          </div>
        </main>
      </div>

      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
      <ShortcutsDialog open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
    </div>
  );
}
