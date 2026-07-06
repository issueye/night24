import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { classNames } from '../utils/format.js';

export function MarkdownMessage({ className, size = 'md', text }) {
  return (
    <div className={classNames('markdown-body', `markdown-body-${size}`, className)}>
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
