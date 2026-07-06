import { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { classNames } from '../../utils/format.js';

export function Tree({ empty = '暂无数据', getIcon, isSelected, maxDepth = 1, node, onSelect, size = 'md' }) {
  if (!node) return <div className="empty-block">{empty}</div>;
  return (
    <div className={classNames('ui-tree', `ui-tree-${size}`)}>
      <TreeNode
        depth={0}
        getIcon={getIcon}
        isSelected={isSelected}
        maxDepth={maxDepth}
        node={node}
        onSelect={onSelect}
      />
    </div>
  );
}

function TreeNode({ depth, getIcon, isSelected, maxDepth, node, onSelect }) {
  const [open, setOpen] = useState(depth < 2);
  const children = Array.isArray(node.children) ? node.children : [];
  const isBranch = children.length > 0 || node.kind === 'directory';
  const isExpandable = isBranch && depth < maxDepth && children.length > 0;
  const Icon = getIcon?.(node, open);

  return (
    <div className="tree-node">
      <button
        className={classNames('tree-row', isSelected?.(node) && 'selected')}
        onClick={() => {
          if (isExpandable) setOpen((value) => !value);
          else if (!isBranch) onSelect?.(node);
        }}
        style={{ '--tree-depth': depth }}
        title={node.path || node.name}
        type="button"
      >
        {isExpandable ? (open ? <ChevronDown size={14} /> : <ChevronRight size={14} />) : <span className="tree-row-spacer" />}
        {Icon}
        <span title={node.path}>{node.name || node.path}</span>
      </button>
      {isExpandable && open && children.map((child) => (
        <TreeNode
          depth={depth + 1}
          getIcon={getIcon}
          isSelected={isSelected}
          key={child.path}
          maxDepth={maxDepth}
          node={child}
          onSelect={onSelect}
        />
      ))}
    </div>
  );
}
