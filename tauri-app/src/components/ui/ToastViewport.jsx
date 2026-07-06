import { CheckCircle2, Info, Loader2, XCircle } from 'lucide-react';
import { Toast } from './Toast.jsx';

function toastIcon(toast) {
  if (toast.loading) return <Loader2 className="spin" size={15} />;
  if (toast.tone === 'success') return <CheckCircle2 size={15} />;
  if (toast.tone === 'danger' || toast.tone === 'error') return <XCircle size={15} />;
  return <Info size={15} />;
}

export function ToastViewport({ onDismiss, toasts }) {
  if (!toasts?.length) return null;
  return (
    <div aria-live="polite" className="ui-toast-viewport">
      {toasts.map((toast) => (
        <Toast
          detail={toast.detail}
          icon={toastIcon(toast)}
          key={toast.id}
          loading={toast.loading}
          message={toast.message}
          onClose={() => onDismiss?.(toast.id)}
          tone={toast.tone}
        />
      ))}
    </div>
  );
}
