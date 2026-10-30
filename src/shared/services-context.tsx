import { createContext, useContext, type ReactNode } from 'react';
import type { OrbitServices } from './contracts';

const OrbitServicesContext = createContext<OrbitServices | null>(null);
export function OrbitServicesProvider({ services, children }: { services: OrbitServices; children: ReactNode }) {
  return <OrbitServicesContext.Provider value={services}>{children}</OrbitServicesContext.Provider>;
}
export function useOrbitServices(): OrbitServices {
  const value = useContext(OrbitServicesContext);
  if (!value) throw new Error('useOrbitServices must be used inside OrbitServicesProvider');
  return value;
}
