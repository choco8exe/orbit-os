import type { ActivityAppUsage, ActivityDailyGoal } from './api';

/** Goals for apps with current-range activity are shown in their app detail instead. */
export function dailyGoalsWithoutVisibleAppUsage(goals: ActivityDailyGoal[], apps: ActivityAppUsage[]): ActivityDailyGoal[] {
  const visibleAppIds = new Set(apps.map(app => app.appId));
  return goals.filter(goal => !visibleAppIds.has(goal.appId));
}
