# ORBIT OS

![Logo ORBIT OS](public/orbit-mark.svg)

**Your Digital Universe** — le centre de commandement personnel pour Windows.

[![Dernière version](https://img.shields.io/github/v/release/choco8exe/orbit-os)](https://github.com/choco8exe/orbit-os/releases/latest)
[![Licence MIT](https://img.shields.io/github/license/choco8exe/orbit-os)](LICENSE)

ORBIT OS réunit le suivi du PC, l’historique local du presse-papiers, les automatisations et des intégrations facultatives dans une application de bureau. Il complète Windows sans le remplacer.

Développé par **Choco8exe**, avec l’aide d’**OpenAI Codex** sur plusieurs parties du projet. La direction du produit et une partie du développement sont de Choco8exe.

**Version 1.0.1** · Windows 10/11 · 64 bits

## Télécharger et installer

Télécharge le setup Windows depuis [la dernière version publiée](https://github.com/choco8exe/orbit-os/releases/latest). Pour une installation classique, choisis l’asset NSIS x64 au format `.exe`.

Node.js, Rust et Visual Studio ne sont pas nécessaires pour installer ou utiliser l’application. Ils servent uniquement à compiler le projet. Le setup n’est pas signé par Authenticode ; Windows peut donc afficher un avertissement sur l’éditeur. Si le runtime WebView2 manque, l’installation Tauri peut le télécharger ; cette étape nécessite Internet.

## Modules

| Module | Fonction |
| --- | --- |
| **Mission Control** | Vue du PC, activité récente, widgets personnalisables et scènes orbitales 3D. |
| **Echo** | Historique local du presse-papiers, recherche, favoris, collections et réglages de confidentialité. |
| **Pulsar** | Automatisations locales avec déclencheurs, conditions, actions, simulation et journal. |
| **Core** | Mesures système accessibles pour le processeur, la mémoire, le stockage et le réseau. Les valeurs indisponibles sont signalées. |
| **Timeline** | Événements récents des modules ORBIT. |
| **Intégrations** | Project Shelf pour FL Studio, Stream Deck, Pulse, Game Sessions, Downloads Organizer et Workspaces. Chaque intégration a ses propres permissions. |

Les intégrations proposées dans ORBIT activent des fonctions incluses dans l’application. ORBIT ne télécharge pas et n’exécute pas de code de plugin arbitraire.

## Signaler un bug

ORBIT OS est un jeune projet et peut encore contenir des bugs. Si tu rencontres un problème, vérifie d’abord s’il existe déjà, puis [ouvre une issue](https://github.com/choco8exe/orbit-os/issues/new?template=bug_report.yml) sur GitHub. Indique la version d’ORBIT, ta version de Windows, les étapes pour reproduire le problème et ce qui s’est passé.

Une **issue** sert à signaler un bug ou à proposer une amélioration. Une **Pull Request** sert à proposer un changement de code ou de documentation. Tu peux signaler un bug sans préparer de Pull Request. Avant de joindre un journal ou une capture, retire les clés, mots de passe, chemins privés et contenus du presse-papiers.

## Compiler depuis les sources

Prérequis de développement :

- Windows 10 ou 11 en 64 bits et le runtime WebView2 ;
- Node.js 22 ou ultérieur ;
- Rust stable avec la cible MSVC ;
- Visual Studio Build Tools avec la charge de travail C++.

Dans PowerShell, depuis le dossier du projet :

```powershell
npm ci
cd streamdeck-companion
npm ci
npm run package
cd ..
npm run tauri dev
```

Pour construire les installateurs Windows :

```powershell
npm run tauri -- build --bundles nsis,msi
```

Les fichiers compilés se trouvent dans `src-tauri/target/release/bundle/`. `npm run dev` ouvre l’interface dans un navigateur pour le développement, mais ne fournit pas les services Windows natifs, comme la surveillance du presse-papiers.

## Données et permissions

- Echo et Pulsar stockent leurs données sur le PC. Le contenu du presse-papiers peut être sensible ; Echo propose une pause, des exclusions et une durée de conservation réglable.
- Pulse suit localement l’activité des applications. Il ne relève ni les sites Web visités ni les contenus consultés.
- Pulsar demande confirmation pour les opérations de fichiers qui l’exigent.
- Les mesures matérielles dépendent de Windows, des pilotes et du matériel ; ORBIT indique quand une valeur n’est pas accessible.
- Les intégrations expliquent les permissions nécessaires avant leur activation.

## Documentation

- [Contrats entre modules](docs/contracts.md)
- [Modules Echo et Pulsar](docs/modules/echo.md) · [Pulsar](docs/modules/pulsar.md)
- [Plugins et permissions](docs/plugins.md)
- [Plateforme de plugins](docs/plugin-platform.md)

## Licence

ORBIT OS est distribué sous licence [MIT](LICENSE). Tu peux utiliser, modifier et redistribuer le code en conservant l’avis de copyright et la licence. Les bibliothèques et ressources tierces conservent leurs propres licences.

## Crédits

ORBIT OS a été développé par **Choco8exe**, avec l’assistance d’**OpenAI Codex** sur plusieurs volets — notamment la conception, le développement et la vérification. Le produit n’a pas été entièrement développé par Codex.
