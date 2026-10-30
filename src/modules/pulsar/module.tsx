import type { ModuleContribution, ModulePageProps } from '../../shared/contracts';
import type { OrbitEventBus } from '../../shared/contracts';
import { useOrbitServices } from '../../shared/services-context';
import { PulsarPage } from './PulsarPage';
import { pulsarPort, pulsarService } from './service';

export const pulsarContribution: ModuleContribution = {
  id: 'pulsar', label: 'Pulsar', description: 'Automatisations locales', icon: '⇢',
  Page: PulsarModulePage,
};

function PulsarModulePage(_props: ModulePageProps) {
  useOrbitServices();
  return <PulsarPage />;
}

/** Core calls once at startup so triggers remain active while another module is visible. */
export function connectPulsarEvents(events: OrbitEventBus): () => void {
  const unsubscribers = [
    events.on('core.alert', () => { void pulsarPort.fireEvent('core.alert').catch(() => undefined); }),
    events.on('echo.entry-added', () => { void pulsarPort.fireEvent('echo.entry-added').catch(() => undefined); }),
  ];
  return () => unsubscribers.forEach(unsubscribe => unsubscribe());
}

export { pulsarService };
