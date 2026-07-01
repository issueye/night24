import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

export function MarkdownMessage({ text }) {
  return (
    <div className="markdown-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a({ href, children, ...props }) {
            return (
              <a href={href} target="_blank" rel="noreferrer" {...props}>
                {children}
              </a>
            );
          },
          code({ inline, className, children, ...props }) {
            if (inline) {
              return <code className="inline-code" {...props}>{children}</code>;
            }
            return (
              <pre className="code-block">
                <code className={className} {...props}>{children}</code>
              </pre>
            );
          },
          table({ children }) {
            return <div className="table-wrap"><table>{children}</table></div>;
          },
        }}
      >
        {text || ''}
      </ReactMarkdown>
    </div>
  );
}
