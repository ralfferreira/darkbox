import { Link, Route, Routes } from 'react-router-dom';
import { ApiError, errorMessage } from './api/client';
import { useSession } from './api/queries';
import { useAuth } from './components/AuthContext';
import { AppShell } from './components/AppShell';
import { AuthScreen } from './pages/AuthScreen';
import { AssessmentOverviewPage } from './pages/AssessmentOverview';
import { AssessmentsPage } from './pages/Assessments';
import { CapabilitiesPage } from './pages/Capabilities';
import { CompaniesPage } from './pages/Companies';
import { CompanyDetailPage } from './pages/CompanyDetail';
import { EvidencePage } from './pages/Evidence';
import { FindingsPage } from './pages/Findings';
import { FlowsPage } from './pages/Flows';
import { InventoryPage } from './pages/Inventory';
import { MemoryPage } from './pages/Memory';
import { ProceduresPage } from './pages/Procedures';
import { RunDetailPage } from './pages/RunDetail';
import { RunsPage } from './pages/Runs';
import { ScopePage } from './pages/Scope';
import { WorkspaceOverviewPage } from './pages/WorkspaceOverview';

function BootScreen({ label }: { label: string }) {
  return (
    <div className="boot-screen" aria-busy="true" aria-live="polite">
      <span className="brand-word mono">DARKBOX</span>
      <span className="boot-label">{label}</span>
      <span className="skeleton boot-skeleton" aria-hidden="true" />
    </div>
  );
}

function AuthGate() {
  const { token, signOut } = useAuth();
  const session = useSession(token);

  if (!token) return <AuthScreen />;
  if (session.isPending) return <BootScreen label="Connecting to workspace" />;

  if (session.isError) {
    if (session.error instanceof ApiError && session.error.status === 401) {
      return <AuthScreen />;
    }
    return (
      <div className="auth-screen">
        <div className="auth-card">
          <div className="auth-kicker mono">connection failed</div>
          <h1>API unreachable</h1>
          <p className="auth-copy">{errorMessage(session.error)}</p>
          <div className="auth-actions">
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void session.refetch()}
            >
              Retry
            </button>
            <button type="button" className="btn" onClick={signOut}>
              Use a different token
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<WorkspaceOverviewPage />} />
        <Route path="companies" element={<CompaniesPage />} />
        <Route path="companies/:id" element={<CompanyDetailPage />} />
        <Route path="assessments" element={<AssessmentsPage />} />
        <Route path="assessments/:id" element={<AssessmentOverviewPage />} />
        <Route path="assessments/:id/inventory" element={<InventoryPage />} />
        <Route path="assessments/:id/flows" element={<FlowsPage />} />
        <Route path="assessments/:id/runs" element={<RunsPage />} />
        <Route path="assessments/:id/runs/:runId" element={<RunDetailPage />} />
        <Route path="assessments/:id/evidence" element={<EvidencePage />} />
        <Route path="assessments/:id/memory" element={<MemoryPage />} />
        <Route path="assessments/:id/findings" element={<FindingsPage />} />
        <Route path="assessments/:id/scope" element={<ScopePage />} />
        <Route path="assessments/:id/procedures" element={<ProceduresPage />} />
        <Route path="capabilities" element={<CapabilitiesPage />} />
        <Route
          path="*"
          element={
            <div className="not-found">
              <div className="page-meta mono">not found</div>
              <h1>This route does not exist.</h1>
              <p>Return to the workspace overview to continue.</p>
              <Link className="btn" to="/">
                Workspace overview
              </Link>
            </div>
          }
        />
      </Route>
    </Routes>
  );
}

export function App() {
  return <AuthGate />;
}
