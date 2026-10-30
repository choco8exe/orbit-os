import test from 'node:test';
import assert from 'node:assert/strict';
import { dailyGoalsWithoutVisibleAppUsage } from './daily-goals.ts';

const goal = (appId, appName = appId) => ({ appId, appName, dailyTargetMinutes: 60 });
const app = appId => ({ appId, name: appId, category: 'other', totalSeconds: 0, backgroundSeconds: 0, sessionCount: 0, dailySeconds: {}, dailyBackgroundSeconds: {}, dailyTargetMinutes: 60 });

test('does not repeat a daily goal already shown in an app detail', () => {
  assert.deepEqual(dailyGoalsWithoutVisibleAppUsage([goal('game.exe'), goal('editor.exe')], [app('game.exe')]), [goal('editor.exe')]);
});

test('keeps goals for apps without range activity without fabricating app usage rows', () => {
  const savedGoals = [goal('removed.exe', 'Removed App')];
  const usageRows = [];
  assert.deepEqual(dailyGoalsWithoutVisibleAppUsage(savedGoals, usageRows), savedGoals);
  assert.deepEqual(usageRows, []);
});
