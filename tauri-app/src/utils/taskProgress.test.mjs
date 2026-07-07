import assert from 'node:assert/strict';
import test from 'node:test';

import {
  deriveTaskProgress,
  extractCompletionReport,
  extractTaskLists,
  isTaskProgressMessage,
} from './taskProgress.js';

test('extractTaskLists parses Chinese headed markdown checkboxes', () => {
  const lists = extractTaskLists(`
## 任务列表
- [ ] 分析需求
- [x] 编写计划
- [完成] 验证结果
`);

  assert.equal(lists.length, 1);
  assert.equal(lists[0].length, 3);
  assert.equal(lists[0][0].title, '分析需求');
  assert.equal(lists[0][0].completed, false);
  assert.equal(lists[0][1].completed, true);
  assert.equal(lists[0][2].completed, true);
});

test('extractTaskLists parses English task headings and numbered items', () => {
  const lists = extractTaskLists(`
## Task List
1. [x] Inspect current flow
2. [ ] Add UI
`);

  assert.equal(lists.length, 1);
  assert.deepEqual(
    lists[0].map((task) => [task.title, task.completed]),
    [
      ['Inspect current flow', true],
      ['Add UI', false],
    ],
  );
});

test('deriveTaskProgress uses the latest assistant task list update', () => {
  const progress = deriveTaskProgress([
    {
      role: 'assistant',
      created_at: '2026-07-07T01:00:00Z',
      content: [{ type: 'text', text: '## 任务列表\n- [ ] A\n- [ ] B' }],
    },
    {
      role: 'user',
      content: [{ type: 'text', text: 'continue' }],
    },
    {
      role: 'assistant',
      created_at: '2026-07-07T01:05:00Z',
      content: [{ type: 'text', text: '## 任务列表\n- [x] A\n- [ ] B' }],
    },
  ]);

  assert.equal(progress.total, 2);
  assert.equal(progress.completed, 1);
  assert.equal(progress.tasks[0].completed, true);
  assert.equal(progress.tasks[1].completed, false);
  assert.equal(progress.updatedAt, '2026-07-07T01:05:00Z');
});

test('deriveTaskProgress resets old tasks and report after a new user message', () => {
  const progress = deriveTaskProgress([
    {
      role: 'assistant',
      created_at: '2026-07-07T01:00:00Z',
      content: [{ type: 'text', text: '## 任务列表\n- [x] A\n- [x] B\n\n## 完成报告\nold report' }],
    },
    {
      role: 'user',
      created_at: '2026-07-07T01:15:00Z',
      content: [{ type: 'text', text: '开始新任务' }],
    },
  ], true);

  assert.equal(progress.hasProgress, false);
  assert.equal(progress.total, 0);
  assert.equal(progress.completed, 0);
  assert.equal(progress.report, '');
  assert.equal(progress.isRunning, true);
});

test('deriveTaskProgress preserves latest tasks and extracts completion report', () => {
  const progress = deriveTaskProgress([
    {
      role: 'assistant',
      created_at: '2026-07-07T01:00:00Z',
      content: [{ type: 'text', text: '## 任务列表\n- [x] A\n- [x] B' }],
    },
    {
      role: 'assistant',
      created_at: '2026-07-07T01:10:00Z',
      content: [{ type: 'text', text: '## 完成报告\n- 已完成: A 和 B\n- 验证: 通过' }],
    },
  ]);

  assert.equal(progress.total, 2);
  assert.equal(progress.completed, 2);
  assert.equal(progress.isComplete, true);
  assert.match(progress.report, /已完成/);
  assert.equal(progress.updatedAt, '2026-07-07T01:10:00Z');
});

test('deriveTaskProgress reads task lists from tool responses', () => {
  const progress = deriveTaskProgress([
    {
      role: 'tool',
      created_at: '2026-07-07T01:00:00Z',
      content: [{
        type: 'tool_response',
        content: '## 任务列表\n- [x] A\n- [ ] B',
      }],
    },
  ]);

  assert.equal(progress.total, 2);
  assert.equal(progress.completed, 1);
  assert.equal(progress.tasks[0].completed, true);
  assert.equal(progress.tasks[1].completed, false);
  assert.equal(progress.updatedAt, '2026-07-07T01:00:00Z');
});

test('extractCompletionReport returns the latest report section', () => {
  const report = extractCompletionReport(`
## 完成报告
old

## Other
ignored

## Final Report
new report
`);

  assert.equal(report, 'new report');
});

test('isTaskProgressMessage detects task list and report messages', () => {
  assert.equal(isTaskProgressMessage({
    role: 'assistant',
    content: [{ type: 'text', text: '## 任务列表\n- [x] 完成测试' }],
  }), true);

  assert.equal(isTaskProgressMessage({
    role: 'tool',
    content: [{ type: 'tool_response', content: '## 完成报告\n已完成构建' }],
  }), true);

  assert.equal(isTaskProgressMessage({
    role: 'assistant',
    content: [{ type: 'text', text: '普通回复内容' }],
  }), false);
});
