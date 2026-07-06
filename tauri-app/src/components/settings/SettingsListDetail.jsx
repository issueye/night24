import { classNames } from '../../utils/format.js';

export function SettingsListDetail({
  managerClassName,
  listClassName,
  listLabel,
  listTitle,
  listActions,
  listExtra,
  listChildren,
  detailClassName,
  children,
}) {
  return (
    <div className={managerClassName}>
      <aside className={classNames('provider-list', listClassName)} aria-label={listLabel}>
        <div className="provider-list-head">
          <strong>{listTitle}</strong>
          {listActions}
        </div>
        {listExtra}
        <div className="provider-list-scroll">
          {listChildren}
        </div>
      </aside>

      <div className={detailClassName}>
        {children}
      </div>
    </div>
  );
}
