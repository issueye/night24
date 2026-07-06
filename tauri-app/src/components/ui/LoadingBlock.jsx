import { Loader2 } from 'lucide-react';
import { classNames } from '../../utils/format.js';

export function LoadingBlock({ className, label = '加载中', size = 'md' }) {
  return (
    <div className={classNames('ui-loading-block', `ui-loading-block-${size}`, className)}>
      <Loader2 className="spin" size={16} />
      <span>{label}</span>
    </div>
  );
}
