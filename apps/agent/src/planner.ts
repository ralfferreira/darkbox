import type { BrowserObservation, BrowserResult, DarkBoxApi } from './darkbox.js';

interface PlannerInput {
  api: DarkBoxApi;
  assessmentId: string;
  runId: string;
  objective: string;
  identityId: string | null;
  startUrl: string;
  shouldStop: () => Promise<string | null>;
  emit: (type: string, payload: Record<string, unknown>) => Promise<unknown>;
}

interface PlannerResult {
  outcome: 'succeeded' | 'partial' | 'blocked' | 'inconclusive';
  summary: string;
  observedResult: string;
  evidenceIds: string[];
  assertion?: { kind: 'text_contains'; value: string };
}

export class PlannerStopped extends Error {}

function observationOf(result: BrowserResult): BrowserObservation | undefined {
  return result.result.observation;
}

function evidenceOf(result: BrowserResult): string[] {
  return result.result.evidence_ids ?? [];
}

function textMatch(item: { name?: string; text?: string; value?: string }, patterns: RegExp[]) {
  const haystack = [item.name, item.text, item.value].filter(Boolean).join(' ').toLowerCase();
  return patterns.some((pattern) => pattern.test(haystack));
}

export async function runDeterministicPlanner(input: PlannerInput): Promise<PlannerResult> {
  const { api, assessmentId, runId, objective, shouldStop } = input;
  const evidence: string[] = [];
  const check = async () => {
    const state = await shouldStop();
    if (state) throw new PlannerStopped(state);
  };
  const remember = (result: BrowserResult) => {
    for (const id of evidenceOf(result)) evidence.push(id);
  };
  const decide = (summary: string) => input.emit('agent.decision', { summary });

  const identities = await api.identities(assessmentId);
  const identity =
    identities.find((item) => item.id === input.identityId) ??
    identities.find((item) => item.role_label === 'admin') ??
    identities[0];
  if (!identity) {
    return {
      outcome: 'blocked',
      summary: 'No configured identity exists for this assessment.',
      observedResult: '',
      evidenceIds: evidence,
    };
  }
  const tenant = identity.tenant_keys[0] ?? null;
  await check();
  await decide(`Open the starting URL with the configured identity ${identity.label}.`);
  let opened = await api.browserOpen(runId, {
    url: input.startUrl,
    identity_id: identity.id,
    tenant,
    decision: `Open ${input.startUrl} with identity ${identity.label}.`,
  });
  remember(opened);
  let observation = observationOf(opened);
  if (!observation) {
    return {
      outcome: 'blocked',
      summary: `The starting page did not produce an observation: ${opened.result.summary}`,
      observedResult: '',
      evidenceIds: evidence,
    };
  }

  await input.emit('agent.browser_session', {
    identity: identity.label,
    tenant,
    url: observation.url,
    title: observation.title,
    engine: opened.result.engine ?? null,
  });

  const clickable = (needle: RegExp) =>
    observation?.interactive.find(
      (item) =>
        item.visible &&
        !item.disabled &&
        item.tag === 'a' &&
        textMatch(
          { name: item.name, text: (item as unknown as { text?: string }).text },
          [needle],
        ),
    );

  if (observation.auth.login_form) {
    await decide('The starting page presents a login form; authenticate with the configured identity.');
    const login = await api.browserLogin(
      runId,
      opened.result.session!.context_key,
      {},
      'Submit the observed login form. The platform injects the identity secret.',
    );
    remember(login);
    observation = observationOf(login);
    if (login.result.login_state !== 'authenticated') {
      return {
        outcome: 'blocked',
        summary: `Authentication was not accepted: ${login.result.summary}`,
        observedResult: observation?.auth.error_text ?? '',
        evidenceIds: evidence,
      };
    }
  } else {
    const signIn = clickable(/sign in|log in|login|entrar/i);
    if (signIn) {
      await decide('Follow the visible sign-in control before exploring the authenticated area.');
      const clicked = await api.browserAct(
        runId,
        opened.result.session!.context_key,
        { type: 'click', ref: signIn.ref },
        `Click ${signIn.name || 'the sign-in control'}.`,
      );
      remember(clicked);
      observation = observationOf(clicked);
      if (observation?.auth.login_form) {
        const login = await api.browserLogin(
          runId,
          opened.result.session!.context_key,
          {},
          'Submit the observed login form with the configured identity.',
        );
        remember(login);
        observation = observationOf(login);
        if (login.result.login_state !== 'authenticated') {
          return {
            outcome: 'blocked',
            summary: `Authentication was not accepted: ${login.result.summary}`,
            observedResult: observation?.auth.error_text ?? '',
            evidenceIds: evidence,
          };
        }
      }
    }
  }

  if (!observation) {
    return {
      outcome: 'inconclusive',
      summary: 'The page was no longer observable after authentication.',
      observedResult: '',
      evidenceIds: evidence,
    };
  }

  const reports = clickable(/reports?|relatórios?/i);
  if (!reports) {
    return {
      outcome: 'partial',
      summary: `Authenticated as ${identity.label}, but no visible reports control was found.`,
      observedResult: observation.headings.join(' · '),
      evidenceIds: evidence,
    };
  }
  await decide(`Open the visible ${reports.name || 'reports'} area.`);
  let moved = await api.browserAct(
    runId,
    opened.result.session!.context_key,
    { type: 'click', ref: reports.ref },
    `Click ${reports.name || 'the reports control'}.`,
  );
  remember(moved);
  observation = observationOf(moved);
  if (!observation) {
    return {
      outcome: 'partial',
      summary: 'The reports area did not produce an observation.',
      observedResult: '',
      evidenceIds: evidence,
    };
  }

  const period = objective.match(/20\d{2}-\d{2}/)?.[0];
  const marginMatch = objective.match(/(?:margin|margem)[^0-9]{0,24}(\d+(?:[.,]\d+)?)/i);
  const margin = marginMatch ? Number(marginMatch[1].replace(',', '.')) : null;
  if (!period || margin === null) {
    return {
      outcome: 'partial',
      summary:
        'The reports area was reached, but the objective did not contain a period and minimum margin to apply.',
      observedResult: observation.headings.join(' · '),
      evidenceIds: evidence,
    };
  }
  const sessionKey = opened.result.session!.context_key;
  const periodControl = observation.interactive.find(
    (item) =>
      item.visible &&
      item.tag === 'select' &&
      (textMatch({ name: item.name }, [/period|período/i]) ||
        item.options?.some((option) => option.value === period)),
  );
  const marginControl = observation.interactive.find(
    (item) =>
      item.visible &&
      (item.tag === 'input' || item.tag === 'textarea') &&
      (textMatch({ name: item.name }, [/margin|margem/i]) ||
        (item.type === 'number' && textMatch({ name: item.name }, [/min/i]))),
  );
  const applyControl = observation.interactive.find(
    (item) =>
      item.visible &&
      !item.disabled &&
      (item.tag === 'button' || item.type === 'submit') &&
      textMatch({ name: item.name }, [/apply|filter|aplicar|filtrar/i]),
  );
  if (!periodControl || !marginControl || !applyControl) {
    return {
      outcome: 'partial',
      summary:
        'The reports page was reached, but the period, minimum margin or apply control was not discoverable.',
      observedResult: observation.headings.join(' · '),
      evidenceIds: evidence,
    };
  }

  await decide(`Apply period ${period} and minimum margin ${margin} using the observed filter controls.`);
  const selected = await api.browserAct(
    runId,
    sessionKey,
    { type: 'select', ref: periodControl.ref, value: period },
    `Select period ${period}.`,
  );
  remember(selected);
  observation = observationOf(selected) ?? observation;
  const filled = await api.browserAct(
    runId,
    sessionKey,
    { type: 'fill', ref: marginControl.ref, value: String(margin) },
    `Set the minimum margin to ${margin}.`,
  );
  remember(filled);
  observation = observationOf(filled) ?? observation;
  const applied = await api.browserAct(
    runId,
    sessionKey,
    { type: 'click', ref: applyControl.ref },
    'Apply the report filter and wait for the rendered result.',
  );
  remember(applied);
  observation = observationOf(applied);
  if (!observation) {
    return {
      outcome: 'inconclusive',
      summary: 'Applying the filter did not produce an observation.',
      observedResult: '',
      evidenceIds: evidence,
    };
  }
  await api.browserAct(
    runId,
    sessionKey,
    { type: 'wait', timeout_ms: 1200 },
    'Wait for the JavaScript-rendered report result.',
  ).catch(() => undefined);
  observation = (await api.browserObserve(runId, sessionKey, 'Read the rendered filter result.')).result
    .observation ?? observation;
  const screenshot = await api.browserScreenshot(runId, sessionKey);
  remember(screenshot);
  const traffic = await api.browserTraffic(runId, sessionKey);
  remember(traffic);

  const statusLine =
    observation.text
      .split('\n')
      .map((line) => line.trim())
      .find((line) => /report rows match period/i.test(line)) ?? '';
  const table = observation.tables[0];
  const rows = table?.rows ?? [];
  const matchingPeriod = rows.filter((row) => row[0] === period);
  const matchingMargin = matchingPeriod.filter((row) => {
    const parsed = Number((row[2] ?? '').replace('%', ''));
    return Number.isFinite(parsed) && parsed >= margin;
  });
  const confirmed =
    statusLine.includes(period) &&
    statusLine.includes(`margin at least ${margin.toFixed(1)}%`) &&
    matchingMargin.length > 0 &&
    matchingMargin.length === matchingPeriod.length;

  const observedResult = [statusLine, ...matchingMargin.map((row) => row.join(' · '))]
    .filter(Boolean)
    .join('\n');
  if (!confirmed) {
    return {
      outcome: 'inconclusive',
      summary: `The filter was applied, but the rendered result did not confirm period ${period} and margin ${margin}.`,
      observedResult,
      evidenceIds: evidence,
    };
  }
  return {
    outcome: 'succeeded',
    summary: `Authenticated as ${identity.label}, opened the reports area, applied period ${period} with minimum margin ${margin}, and confirmed the rendered result.`,
    observedResult,
    evidenceIds: [...new Set(evidence)],
    assertion: { kind: 'text_contains', value: statusLine },
  };
}
