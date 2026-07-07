import { messageText } from './format.js';

const TASK_HEADING = /^(任务列表|任务清单|步骤任务|执行计划|task list|tasks|plan)$/i;
const REPORT_HEADING = /^(完成报告|completion report|final report)$/i;
const MARKDOWN_HEADING = /^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$/;
const TASK_ITEM = /^\s*(?:[-*+]|\d+[.)])\s+\[([^\]]*)\]\s+(.+?)\s*$/;

function cleanHeading(value) {
  return String(value || '')
    .replace(/[:：]\s*$/, '')
    .trim();
}

function isCompletedMark(value) {
  const mark = String(value || '').trim().toLowerCase();
  return mark === 'x' || mark === 'done' || mark === 'completed' || mark === 'complete' || mark === '✓' || mark === '✔' || mark === '完成';
}

function parseTasksFromLines(lines) {
  return lines
    .map((line, index) => {
      const match = line.match(TASK_ITEM);
      if (!match) return null;
      return {
        id: `task-${index}-${match[2].trim().toLowerCase().replace(/\s+/g, '-')}`,
        title: match[2].trim(),
        completed: isCompletedMark(match[1]),
      };
    })
    .filter(Boolean);
}

function findSections(text, headingPattern) {
  const lines = String(text || '').split(/\r?\n/);
  const sections = [];
  let active = null;

  lines.forEach((line) => {
    const heading = line.match(MARKDOWN_HEADING);
    if (heading) {
      if (active) sections.push(active);
      const label = cleanHeading(heading[1]);
      active = headingPattern.test(label) ? { heading: label, lines: [] } : null;
      return;
    }

    if (active) {
      active.lines.push(line);
    }
  });

  if (active) sections.push(active);
  return sections;
}

export function extractTaskLists(text) {
  const sections = findSections(text, TASK_HEADING)
    .map((section) => parseTasksFromLines(section.lines))
    .filter((tasks) => tasks.length > 0);

  if (sections.length > 0) return sections;

  const fallbackTasks = parseTasksFromLines(String(text || '').split(/\r?\n/));
  return fallbackTasks.length > 0 ? [fallbackTasks] : [];
}

export function extractCompletionReport(text) {
  const sections = findSections(text, REPORT_HEADING)
    .map((section) => section.lines.join('\n').trim())
    .filter(Boolean);
  return sections.at(-1) || '';
}

export function deriveTaskProgress(messages = [], isRunning = false) {
  const visibleMessages = messages.slice(lastUserMessageIndex(messages) + 1);
  let tasks = [];
  let report = '';
  let updatedAt = '';

  visibleMessages.forEach((message) => {
    const text = taskProgressTextFromMessage(message);
    if (!text) return;
    const taskLists = extractTaskLists(text);
    if (taskLists.length > 0) {
      tasks = taskLists.at(-1);
      updatedAt = message.created_at || message.createdAt || updatedAt;
    }

    const completionReport = extractCompletionReport(text);
    if (completionReport) {
      report = completionReport;
      updatedAt = message.created_at || message.createdAt || updatedAt;
    }
  });

  const completed = tasks.filter((task) => task.completed).length;
  return {
    hasProgress: tasks.length > 0 || Boolean(report),
    tasks,
    report,
    completed,
    total: tasks.length,
    isComplete: tasks.length > 0 && completed === tasks.length,
    isRunning,
    updatedAt,
  };
}

function lastUserMessageIndex(messages) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (String(messages[index]?.role || '').toLowerCase() === 'user') {
      return index;
    }
  }
  return -1;
}

export function isTaskProgressMessage(message) {
  const text = taskProgressTextFromMessage(message);
  if (!text) return false;
  return extractTaskLists(text).length > 0 || Boolean(extractCompletionReport(text));
}

function taskProgressTextFromMessage(message) {
  const role = String(message?.role || '').toLowerCase();
  if (role !== 'assistant' && role !== 'tool') return '';
  return role === 'tool' ? taskTextFromToolMessage(message) : messageText(message);
}

function taskTextFromToolMessage(message) {
  if (!Array.isArray(message?.content)) return messageText(message);
  return message.content
    .map((block) => {
      if (block?.type === 'tool_response') return block.content || '';
      if (block?.type === 'text') return block.text || '';
      return '';
    })
    .filter(Boolean)
    .join('\n\n');
}
