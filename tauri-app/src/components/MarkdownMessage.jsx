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
          code({ node, className, children, ...props }) {
            const isBlock = node?.position?.start?.line !== node?.position?.end?.line || Boolean(className);
            return (
              <code className={isBlock ? className : 'inline-code'} {...props}>
                {children}
              </code>
            );
          },
          pre({ children }) {
            return <pre className="code-block">{children}</pre>;
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
