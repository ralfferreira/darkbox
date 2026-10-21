import type { ReactNode } from 'react';

type Block =
  | { type: 'heading'; level: number; text: string }
  | { type: 'paragraph'; text: string }
  | { type: 'code'; lang: string; text: string }
  | { type: 'list'; ordered: boolean; items: string[] }
  | { type: 'quote'; lines: string[] }
  | { type: 'rule' };

const HEADING = /^(#{1,6})\s+(.*)$/;
const LIST_ITEM = /^\s*(?:([-*+])|(\d+)\.)\s+(.*)$/;
const RULE = /^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/;

function parseBlocks(markdown: string): Block[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const blocks: Block[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (line.trim() === '') {
      index += 1;
      continue;
    }

    const fence = line.match(/^\s*```([\w+-]*)\s*$/);
    if (fence) {
      const lang = fence[1] ?? '';
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !/^\s*```\s*$/.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      index += 1;
      blocks.push({ type: 'code', lang, text: code.join('\n') });
      continue;
    }

    const heading = line.match(HEADING);
    if (heading) {
      blocks.push({
        type: 'heading',
        level: heading[1].length,
        text: heading[2],
      });
      index += 1;
      continue;
    }

    if (RULE.test(line)) {
      blocks.push({ type: 'rule' });
      index += 1;
      continue;
    }

    if (/^\s*>/.test(line)) {
      const quote: string[] = [];
      while (index < lines.length && /^\s*>/.test(lines[index])) {
        quote.push(lines[index].replace(/^\s*>\s?/, ''));
        index += 1;
      }
      blocks.push({ type: 'quote', lines: quote });
      continue;
    }

    if (LIST_ITEM.test(line)) {
      const first = line.match(LIST_ITEM);
      const ordered = Boolean(first?.[2]);
      const items: string[] = [];
      while (index < lines.length) {
        const match = lines[index].match(LIST_ITEM);
        if (!match) break;
        items.push(match[3]);
        index += 1;
      }
      blocks.push({ type: 'list', ordered, items });
      continue;
    }

    const paragraph: string[] = [];
    while (index < lines.length) {
      const current = lines[index];
      if (
        current.trim() === '' ||
        HEADING.test(current) ||
        LIST_ITEM.test(current) ||
        RULE.test(current) ||
        /^\s*>/.test(current) ||
        /^\s*```/.test(current)
      ) {
        break;
      }
      paragraph.push(current.trim());
      index += 1;
    }
    blocks.push({ type: 'paragraph', text: paragraph.join(' ') });
  }

  return blocks;
}

const INLINE =
  /(`[^`]+`)|(\*\*[^*]+\*\*)|(__[^_]+__)|(\*[^*\n]+\*)|(_[^_\n]+_)|(\[[^\]]+\]\((https?:\/\/[^\s)]+)\))|(https?:\/\/[^\s<]+)/g;

function safeExternalUrl(raw: string): string | null {
  try {
    const url = new URL(raw);
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return null;
    return url.toString();
  } catch {
    return null;
  }
}

function renderInline(text: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  INLINE.lastIndex = 0;

  while ((match = INLINE.exec(text)) !== null) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
    }
    const token = match[0];
    const key = `${keyPrefix}-${match.index}`;

    if (match[1]) {
      nodes.push(<code key={key}>{token.slice(1, -1)}</code>);
    } else if (match[2] || match[3]) {
      nodes.push(<strong key={key}>{token.slice(2, -2)}</strong>);
    } else if (match[4] || match[5]) {
      nodes.push(<em key={key}>{token.slice(1, -1)}</em>);
    } else if (match[6]) {
      const label = match[6].slice(1, match[6].indexOf(']'));
      const url = safeExternalUrl(match[7]);
      if (url) {
        nodes.push(
          <a key={key} href={url} target="_blank" rel="noreferrer noopener">
            {label}
          </a>,
        );
      } else {
        nodes.push(token);
      }
    } else if (match[8]) {
      const url = safeExternalUrl(match[8]);
      if (url) {
        nodes.push(
          <a key={key} href={url} target="_blank" rel="noreferrer noopener">
            {match[8]}
          </a>,
        );
      } else {
        nodes.push(token);
      }
    }

    lastIndex = match.index + token.length;
  }

  if (lastIndex < text.length) nodes.push(text.slice(lastIndex));
  return nodes;
}

export function SafeMarkdown({ markdown }: { markdown: string }) {
  const blocks = parseBlocks(markdown);

  return (
    <div className="markdown">
      {blocks.map((block, blockIndex) => {
        const key = `block-${blockIndex}`;
        if (block.type === 'heading') {
          const level = Math.min(6, Math.max(3, block.level + 2));
          const Tag = `h${level}` as 'h3' | 'h4' | 'h5' | 'h6';
          return <Tag key={key}>{renderInline(block.text, key)}</Tag>;
        }
        if (block.type === 'paragraph') {
          return <p key={key}>{renderInline(block.text, key)}</p>;
        }
        if (block.type === 'rule') {
          return <hr key={key} />;
        }
        if (block.type === 'code') {
          return (
            <div className="md-code" key={key}>
              {block.lang ? <span className="md-code-lang mono">{block.lang}</span> : null}
              <pre>
                <code>{block.text}</code>
              </pre>
            </div>
          );
        }
        if (block.type === 'quote') {
          return (
            <blockquote key={key}>
              {block.lines.map((line, lineIndex) => (
                <p key={`${key}-${lineIndex}`}>{renderInline(line, `${key}-${lineIndex}`)}</p>
              ))}
            </blockquote>
          );
        }
        if (block.ordered) {
          return (
            <ol key={key}>
              {block.items.map((item, itemIndex) => (
                <li key={`${key}-${itemIndex}`}>
                  {renderInline(item, `${key}-${itemIndex}`)}
                </li>
              ))}
            </ol>
          );
        }
        return (
          <ul key={key}>
            {block.items.map((item, itemIndex) => (
              <li key={`${key}-${itemIndex}`}>{renderInline(item, `${key}-${itemIndex}`)}</li>
            ))}
          </ul>
        );
      })}
    </div>
  );
}
