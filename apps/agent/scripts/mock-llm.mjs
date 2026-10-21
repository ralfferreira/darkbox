// Deterministic OpenAI-compatible model for local development. It calls
// inventory_list once, then finish. No network and no provider key.
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
            name: 'inventory_list',
            arguments: { limit: 5 },
            content: 'I will inspect the current inventory first.',
          }
        : {
            name: 'finish',
            arguments: {
              summary:
                'Inventory inspected for the mock run. No target request was made by this step.',
              next_steps: ['Review the candidate endpoints before observing them.'],
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
