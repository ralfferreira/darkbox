// Deterministic OpenAI-compatible model for transport tests. It calls
// identity_list once, then finishes as inconclusive. It is not a browser test
// driver and is never presented as a model.
import http from 'node:http';

let calls = 0;
const server = http.createServer((request, response) => {
  if (request.method !== 'POST' || request.url !== '/v1/chat/completions') {
    response.writeHead(404).end();
    return;
  }
  let raw = '';
  request.on('data', (chunk) => {
    raw += chunk;
  });
  request.on('end', () => {
    calls += 1;
    const toolCall =
      calls === 1
        ? {
            name: 'identity_list',
            arguments: {},
            content: 'I will inspect the configured identities first.',
          }
        : {
            name: 'finish',
            arguments: {
              outcome: 'inconclusive',
              summary:
                'The mock transport test did not run a browser objective. This is not a completed assessment.',
              next_steps: ['Use the deterministic planner or a provider for a functional run.'],
              evidence_ids: [],
            },
            content: null,
          };
    const message = {
      role: 'assistant',
      content: toolCall.content,
      tool_calls: [
        {
          id: `call_${calls}`,
          type: 'function',
          function: {
            name: toolCall.name,
            arguments: JSON.stringify(toolCall.arguments),
          },
        },
      ],
    };
    const body = JSON.stringify({
      id: `mock-${calls}`,
      object: 'chat.completion',
      model: 'mock-agent',
      choices: [{ index: 0, message, finish_reason: 'tool_calls' }],
      usage: { prompt_tokens: 120, completion_tokens: 40, total_tokens: 160 },
    });
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(body);
  });
});
const port = Number(process.env.MOCK_LLM_PORT ?? 9911);
server.listen(port, '127.0.0.1', () => {
  console.log(JSON.stringify({ service: 'mock-llm', port }));
});
