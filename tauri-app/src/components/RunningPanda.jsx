import { classNames } from '../utils/format.js';

export function RunningPanda({ className, label = '正在执行当前任务...', showLabel = true }) {
  return (
    <div className={classNames('running-panda', className)} aria-label={label} role="status">
      <svg className="running-panda-pixel" viewBox="0 0 72 36" aria-hidden="true" shapeRendering="crispEdges">
        <g className="pixel-trail">
          <rect x="3" y="18" width="10" height="2" />
          <rect x="8" y="25" width="12" height="2" />
        </g>
        <rect className="pixel-shadow" x="20" y="31" width="34" height="2" />
        <g className="pixel-panda">
          <g className="pixel-tail">
            <rect className="pixel-russet-dark" x="12" y="17" width="8" height="6" />
            <rect className="pixel-russet" x="14" y="13" width="10" height="6" />
            <rect className="pixel-cream" x="19" y="17" width="4" height="4" />
          </g>
          <rect className="pixel-russet-dark pixel-leg leg-a" x="27" y="26" width="3" height="5" />
          <rect className="pixel-russet-dark pixel-leg leg-b" x="41" y="26" width="3" height="5" />
          <rect className="pixel-russet" x="23" y="15" width="24" height="12" />
          <rect className="pixel-russet-light" x="27" y="13" width="16" height="4" />
          <rect className="pixel-cream" x="30" y="19" width="12" height="6" />
          <rect className="pixel-russet-dark" x="46" y="11" width="4" height="4" />
          <rect className="pixel-russet-dark" x="58" y="11" width="4" height="4" />
          <rect className="pixel-russet" x="49" y="13" width="12" height="12" />
          <rect className="pixel-cream" x="51" y="17" width="8" height="5" />
          <rect className="pixel-ink" x="52" y="18" width="2" height="2" />
          <rect className="pixel-ink" x="58" y="18" width="2" height="2" />
          <rect className="pixel-ink" x="55" y="21" width="2" height="2" />
          <rect className="pixel-russet-dark pixel-leg leg-c" x="32" y="26" width="3" height="5" />
          <rect className="pixel-russet-dark pixel-leg leg-d" x="46" y="25" width="3" height="5" />
        </g>
      </svg>
      {showLabel && <span>{label}</span>}
    </div>
  );
}
