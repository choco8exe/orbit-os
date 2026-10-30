import type { ModuleContribution, ModuleId } from '../shared/contracts';

const modules = new Map<ModuleId, ModuleContribution>();
export function registerModule(module: ModuleContribution): void {
  if (modules.has(module.id)) throw new Error(`ORBIT module already registered: ${module.id}`);
  modules.set(module.id, module);
}
export function getModule(id: ModuleId): ModuleContribution | undefined { return modules.get(id); }
