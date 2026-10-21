import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import { request, requestText } from './client';
import { isTerminalRun } from './types';
import type {
  AssessmentDetail,
  AssessmentSummary,
  CancelRunRequest,
  CapabilitiesDto,
  CompanyDto,
  ContextPackageDto,
  CreateAssessmentRequest,
  CreateCompanyRequest,
  CreateFindingRequest,
  CreateRunRequest,
  EligibilityDto,
  EvidenceDto,
  ExportDto,
  ExportRequest,
  FindingDto,
  FlowDto,
  IdentityDto,
  ImportUrlsRequest,
  InventoryResponse,
  MemoryPageDto,
  MemorySearchRequest,
  ObservationDto,
  ProcedureDto,
  ResumeRunRequest,
  ReviewFindingRequest,
  RunDetail,
  RunSummary,
  ScopeDecision,
  ScopePolicy,
  ScopeVersionDto,
  SessionDto,
  TestResultDto,
} from './types';

export interface InventoryFilters {
  state?: string;
  host?: string;
  identity_id?: string;
  source?: string;
  q?: string;
  limit?: number;
  offset?: number;
}

export const keys = {
  session: ['session'] as const,
  companies: ['companies'] as const,
  company: (id: string) => ['company', id] as const,
  assessments: (companyId: string) => ['assessments', 'company', companyId] as const,
  assessmentsAll: ['assessments', 'all'] as const,
  assessment: (id: string) => ['assessment', id] as const,
  scopeVersions: (id: string) => ['scope-versions', id] as const,
  inventory: (id: string, filters: InventoryFilters) =>
    ['inventory', id, filters] as const,
  observations: (id: string, limit: number) => ['observations', id, limit] as const,
  identities: (id: string) => ['identities', id] as const,
  flows: (id: string) => ['flows', id] as const,
  runs: (id: string, limit: number) => ['runs', id, limit] as const,
  run: (runId: string) => ['run', runId] as const,
  evidence: (id: string, kind: string, limit: number) =>
    ['evidence', id, kind, limit] as const,
  evidenceContent: (evidenceId: string) => ['evidence-content', evidenceId] as const,
  memory: (id: string) => ['memory', id] as const,
  memorySearch: (id: string, query: string) => ['memory-search', id, query] as const,
  context: (id: string, budget: number, identityId: string) =>
    ['context', id, budget, identityId] as const,
  findings: (id: string, state: string) => ['findings', id, state] as const,
  results: (id: string, limit: number) => ['results', id, limit] as const,
  eligibility: (id: string) => ['eligibility', id] as const,
  procedures: ['procedures'] as const,
  capabilities: ['capabilities'] as const,
};

const get = <T>(path: string): Promise<T> => request<T>(path);

export function useSession(token: string | null): UseQueryResult<SessionDto> {
  return useQuery({
    queryKey: keys.session,
    queryFn: () => get<SessionDto>('/session'),
    enabled: Boolean(token),
    retry: false,
    staleTime: 5 * 60_000,
  });
}

export function useCompanies(): UseQueryResult<CompanyDto[]> {
  return useQuery({
    queryKey: keys.companies,
    queryFn: () => get<CompanyDto[]>('/companies'),
    staleTime: 30_000,
  });
}

export function useCompany(companyId: string | undefined): UseQueryResult<CompanyDto> {
  return useQuery({
    queryKey: keys.company(companyId ?? ''),
    queryFn: () => get<CompanyDto>(`/companies/${companyId}`),
    enabled: Boolean(companyId),
    staleTime: 30_000,
  });
}

export function useAssessments(companyId: string | undefined): UseQueryResult<AssessmentSummary[]> {
  return useQuery({
    queryKey: keys.assessments(companyId ?? ''),
    queryFn: () =>
      get<AssessmentSummary[]>(
        `/assessments?company_id=${encodeURIComponent(companyId ?? '')}`,
      ),
    enabled: Boolean(companyId),
    staleTime: 15_000,
  });
}

async function fetchAllAssessments(): Promise<AssessmentSummary[]> {
  const companies = await get<CompanyDto[]>('/companies');
  const lists = await Promise.all(
    companies.map((company) =>
      get<AssessmentSummary[]>(
        `/assessments?company_id=${encodeURIComponent(company.id)}`,
      ),
    ),
  );
  return lists.flat().sort((a, b) => b.last_activity_at.localeCompare(a.last_activity_at));
}

// The API scopes assessments by company, so the workspace view composes them.
export function useAllAssessments(): UseQueryResult<AssessmentSummary[]> {
  return useQuery({
    queryKey: keys.assessmentsAll,
    queryFn: fetchAllAssessments,
    staleTime: 15_000,
  });
}

export function useAssessment(id: string | undefined): UseQueryResult<AssessmentDetail> {
  return useQuery({
    queryKey: keys.assessment(id ?? ''),
    queryFn: () => get<AssessmentDetail>(`/assessments/${id}`),
    enabled: Boolean(id),
    staleTime: 10_000,
    refetchInterval: (query) => {
      const runs = query.state.data?.recent_runs ?? [];
      return runs.some((run) => !isTerminalRun(run.state)) ? 4_000 : false;
    },
  });
}

export function useScopeVersions(id: string | undefined): UseQueryResult<ScopeVersionDto[]> {
  return useQuery({
    queryKey: keys.scopeVersions(id ?? ''),
    queryFn: () => get<ScopeVersionDto[]>(`/assessments/${id}/scope-versions`),
    enabled: Boolean(id),
    staleTime: 30_000,
  });
}

export function useInventory(
  id: string | undefined,
  filters: InventoryFilters,
): UseQueryResult<InventoryResponse> {
  return useQuery({
    queryKey: keys.inventory(id ?? '', filters),
    queryFn: () =>
      request<InventoryResponse>(`/assessments/${id}/inventory`, {
        query: {
          state: filters.state,
          host: filters.host,
          identity_id: filters.identity_id,
          source: filters.source,
          q: filters.q,
          limit: filters.limit,
          offset: filters.offset,
        },
      }),
    enabled: Boolean(id),
    staleTime: 30_000,
    placeholderData: (previous) => previous,
  });
}

export function useObservations(
  id: string | undefined,
  limit = 8,
): UseQueryResult<ObservationDto[]> {
  return useQuery({
    queryKey: keys.observations(id ?? '', limit),
    queryFn: () =>
      get<ObservationDto[]>(`/assessments/${id}/observations?limit=${limit}`),
    enabled: Boolean(id),
    staleTime: 15_000,
  });
}

export function useIdentities(id: string | undefined): UseQueryResult<IdentityDto[]> {
  return useQuery({
    queryKey: keys.identities(id ?? ''),
    queryFn: () => get<IdentityDto[]>(`/assessments/${id}/identities`),
    enabled: Boolean(id),
    staleTime: 60_000,
  });
}

export function useFlows(id: string | undefined): UseQueryResult<FlowDto[]> {
  return useQuery({
    queryKey: keys.flows(id ?? ''),
    queryFn: () => get<FlowDto[]>(`/assessments/${id}/flows`),
    enabled: Boolean(id),
    staleTime: 30_000,
  });
}

export function useRuns(id: string | undefined, limit = 50): UseQueryResult<RunSummary[]> {
  return useQuery({
    queryKey: keys.runs(id ?? '', limit),
    queryFn: () => get<RunSummary[]>(`/assessments/${id}/runs?limit=${limit}`),
    enabled: Boolean(id),
    staleTime: 5_000,
    refetchInterval: (query) => {
      const runs = query.state.data ?? [];
      return runs.some((run) => !isTerminalRun(run.state)) ? 4_000 : false;
    },
  });
}

export function useRun(runId: string | undefined): UseQueryResult<RunDetail> {
  return useQuery({
    queryKey: keys.run(runId ?? ''),
    queryFn: () => get<RunDetail>(`/runs/${runId}`),
    enabled: Boolean(runId),
    staleTime: 2_000,
    refetchInterval: (query) => {
      const run = query.state.data?.run;
      if (!run) return false;
      return isTerminalRun(run.state) && !run.effect_unknown ? false : 3_000;
    },
  });
}

export function useEvidence(
  id: string | undefined,
  kind = '',
  limit = 100,
): UseQueryResult<EvidenceDto[]> {
  return useQuery({
    queryKey: keys.evidence(id ?? '', kind, limit),
    queryFn: () =>
      request<EvidenceDto[]>(`/assessments/${id}/evidence`, {
        query: { kind: kind || undefined, limit },
      }),
    enabled: Boolean(id),
    staleTime: 30_000,
  });
}

export function useEvidenceItem(
  evidenceId: string | undefined,
): UseQueryResult<EvidenceDto> {
  return useQuery({
    queryKey: ['evidence-item', evidenceId ?? ''],
    queryFn: () => get<EvidenceDto>(`/evidence/${evidenceId}`),
    enabled: Boolean(evidenceId),
    staleTime: 30_000,
  });
}

export function useEvidenceContent(
  evidenceId: string | null,
): UseQueryResult<string> {
  return useQuery({
    queryKey: keys.evidenceContent(evidenceId ?? ''),
    queryFn: () => requestText(`/evidence/${evidenceId}/content`),
    enabled: Boolean(evidenceId),
    retry: false,
    staleTime: Infinity,
  });
}

export function useMemory(id: string | undefined): UseQueryResult<MemoryPageDto[]> {
  return useQuery({
    queryKey: keys.memory(id ?? ''),
    queryFn: () => get<MemoryPageDto[]>(`/assessments/${id}/memory`),
    enabled: Boolean(id),
    staleTime: 60_000,
  });
}

export function useMemorySearch(
  id: string | undefined,
  query: string,
): UseQueryResult<MemoryPageDto[]> {
  return useQuery({
    queryKey: keys.memorySearch(id ?? '', query),
    queryFn: () =>
      request<MemoryPageDto[]>(`/assessments/${id}/memory/search`, {
        method: 'POST',
        body: {
          query,
          token_budget: null,
          identity_id: null,
        } satisfies MemorySearchRequest,
      }),
    enabled: Boolean(id) && query.trim().length > 0,
    staleTime: 10_000,
  });
}

export function useContextPackage(
  id: string | undefined,
  tokenBudget: number,
  identityId: string,
  enabled: boolean,
): UseQueryResult<ContextPackageDto> {
  return useQuery({
    queryKey: keys.context(id ?? '', tokenBudget, identityId),
    queryFn: () =>
      request<ContextPackageDto>(`/assessments/${id}/context`, {
        query: {
          token_budget: tokenBudget,
          identity_id: identityId || undefined,
        },
      }),
    enabled: Boolean(id) && enabled,
    staleTime: 30_000,
  });
}

export function useFindings(
  id: string | undefined,
  state = '',
): UseQueryResult<FindingDto[]> {
  return useQuery({
    queryKey: keys.findings(id ?? '', state),
    queryFn: () =>
      request<FindingDto[]>(`/assessments/${id}/findings`, {
        query: { state: state || undefined },
      }),
    enabled: Boolean(id),
    staleTime: 30_000,
  });
}

export function useResults(
  id: string | undefined,
  limit = 50,
): UseQueryResult<TestResultDto[]> {
  return useQuery({
    queryKey: keys.results(id ?? '', limit),
    queryFn: () => get<TestResultDto[]>(`/assessments/${id}/results?limit=${limit}`),
    enabled: Boolean(id),
    staleTime: 15_000,
  });
}

export function useEligibility(id: string | undefined): UseQueryResult<EligibilityDto[]> {
  return useQuery({
    queryKey: keys.eligibility(id ?? ''),
    queryFn: () => get<EligibilityDto[]>(`/assessments/${id}/eligibility`),
    enabled: Boolean(id),
    staleTime: 30_000,
  });
}

export function useProcedures(): UseQueryResult<ProcedureDto[]> {
  return useQuery({
    queryKey: keys.procedures,
    queryFn: () => get<ProcedureDto[]>('/procedures'),
    staleTime: 5 * 60_000,
  });
}

export function useCapabilities(): UseQueryResult<CapabilitiesDto> {
  return useQuery({
    queryKey: keys.capabilities,
    queryFn: () => get<CapabilitiesDto>('/capabilities'),
    staleTime: 60_000,
  });
}

export function useCreateCompany(): UseMutationResult<CompanyDto, Error, CreateCompanyRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateCompanyRequest) =>
      request<CompanyDto>('/companies', { method: 'POST', body }),
    onSuccess: (company) => {
      void queryClient.invalidateQueries({ queryKey: keys.companies });
      void queryClient.invalidateQueries({ queryKey: keys.assessmentsAll });
      void queryClient.invalidateQueries({ queryKey: keys.company(company.id) });
    },
  });
}

export function useCreateAssessment(): UseMutationResult<
  AssessmentSummary,
  Error,
  CreateAssessmentRequest
> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateAssessmentRequest) =>
      request<AssessmentSummary>('/assessments', { method: 'POST', body }),
    onSuccess: (assessment) => {
      void queryClient.invalidateQueries({ queryKey: keys.companies });
      void queryClient.invalidateQueries({ queryKey: keys.assessmentsAll });
      void queryClient.invalidateQueries({
        queryKey: keys.assessments(assessment.company_id),
      });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(assessment.id) });
    },
  });
}

export function useImportUrls(
  assessmentId: string,
): UseMutationResult<RunSummary, Error, ImportUrlsRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: ImportUrlsRequest) =>
      request<RunSummary>(`/assessments/${assessmentId}/import-urls`, {
        method: 'POST',
        body,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.runs(assessmentId, 50) });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(assessmentId) });
      void queryClient.invalidateQueries({ queryKey: ['inventory', assessmentId] });
    },
  });
}

export function useCreateRun(
  assessmentId: string,
): UseMutationResult<RunSummary, Error, CreateRunRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateRunRequest) =>
      request<RunSummary>('/runs', { method: 'POST', body }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.runs(assessmentId, 50) });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(assessmentId) });
    },
  });
}

export function useCancelRun(
  runId: string,
): UseMutationResult<RunSummary, Error, CancelRunRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CancelRunRequest) =>
      request<RunSummary>(`/runs/${runId}/cancel`, { method: 'POST', body }),
    onSuccess: (run) => {
      void queryClient.invalidateQueries({ queryKey: keys.run(runId) });
      void queryClient.invalidateQueries({ queryKey: keys.runs(run.assessment_id, 50) });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(run.assessment_id) });
    },
  });
}

export function useResumeRun(
  runId: string,
): UseMutationResult<RunSummary, Error, ResumeRunRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: ResumeRunRequest) =>
      request<RunSummary>(`/runs/${runId}/resume`, { method: 'POST', body }),
    onSuccess: (run) => {
      void queryClient.invalidateQueries({ queryKey: keys.run(runId) });
      void queryClient.invalidateQueries({ queryKey: keys.runs(run.assessment_id, 50) });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(run.assessment_id) });
    },
  });
}

export function useCreateExport(
  assessmentId: string,
): UseMutationResult<ExportDto, Error, ExportRequest> {
  return useMutation({
    mutationFn: (body: ExportRequest) =>
      request<ExportDto>(`/assessments/${assessmentId}/exports`, {
        method: 'POST',
        body,
      }),
  });
}

export function useCreateFinding(
  assessmentId: string,
): UseMutationResult<FindingDto, Error, CreateFindingRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateFindingRequest) =>
      request<FindingDto>(`/assessments/${assessmentId}/findings`, {
        method: 'POST',
        body,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['findings', assessmentId] });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(assessmentId) });
      void queryClient.invalidateQueries({ queryKey: keys.assessmentsAll });
    },
  });
}

export function useReviewFinding(
  assessmentId: string,
): UseMutationResult<FindingDto, Error, { findingId: string; body: ReviewFindingRequest }> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ findingId, body }) =>
      request<FindingDto>(`/findings/${findingId}/review`, {
        method: 'POST',
        body,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['findings', assessmentId] });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(assessmentId) });
    },
  });
}

export function useCreateScopeVersion(
  assessmentId: string,
): UseMutationResult<ScopeVersionDto, Error, { policy: ScopePolicy; note: string }> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body) =>
      request<ScopeVersionDto>(`/assessments/${assessmentId}/scope-versions`, {
        method: 'POST',
        body,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.scopeVersions(assessmentId) });
      void queryClient.invalidateQueries({ queryKey: keys.assessment(assessmentId) });
      void queryClient.invalidateQueries({ queryKey: ['inventory', assessmentId] });
    },
  });
}

export function usePreviewScope(
  assessmentId: string,
): UseMutationResult<ScopeDecision[], Error, string[]> {
  return useMutation({
    mutationFn: (targets: string[]) =>
      request<ScopeDecision[]>(`/assessments/${assessmentId}/scope-preview`, {
        method: 'POST',
        body: { targets },
      }),
  });
}
