import { useCallback } from 'react';
import { isVisibleChatMessage, messageText } from '../utils/format.js';
import { withMessageText } from '../utils/messages.js';

const findMessageIndex = (items, message) => (message.id ? items.findIndex((item) => item.id === message.id) : -1);

export function useMessageActions(setMessages) {
  const addOrReplaceMessage = useCallback((message) => {
    if (!isVisibleChatMessage(message)) return;
    setMessages((items) => {
      const index = findMessageIndex(items, message);
      if (index < 0) return [...items, message];
      return items.map((item, itemIndex) => (itemIndex === index ? message : item));
    });
  }, [setMessages]);

  const addTypewriterMessage = useCallback((message) => {
    if (!message?.id) {
      addOrReplaceMessage(message);
      return;
    }

    const fullText = messageText(message);
    if (!fullText.trim()) {
      addOrReplaceMessage(message);
      return;
    }

    const baseMessage = withMessageText(message, '');
    setMessages((items) => {
      const index = findMessageIndex(items, message);
      if (index >= 0) return items.map((item, itemIndex) => (itemIndex === index ? message : item));
      return [...items, baseMessage];
    });

    let offset = 0;
    const step = () => {
      offset = Math.min(fullText.length, offset + Math.max(2, Math.ceil(fullText.length / 90)));
      const visibleMessage = withMessageText(message, fullText.slice(0, offset));
      setMessages((items) => items.map((item) => (item.id === message.id ? visibleMessage : item)));
      if (offset < fullText.length) {
        window.setTimeout(step, 16);
      }
    };
    window.setTimeout(step, 16);
  }, [addOrReplaceMessage, setMessages]);

  return { addOrReplaceMessage, addTypewriterMessage };
}
