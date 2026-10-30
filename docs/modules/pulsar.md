# PULSAR — moteur local d’automatisations

## Contrat natif

Fichiers d’implémentation : `src/modules/pulsar/` et `src-tauri/src/modules/pulsar.rs`.
Core doit déclarer `pub mod pulsar`, appeler `pulsar::initialize(&app)` pendant le setup, enregistrer
les commandes ci-dessous dans `generate_handler!`, connecter `pulsarService` au `OrbitServices`, et
appeler `connectPulsarEvents(events)` une seule fois au démarrage. La connexion d’événements au niveau
de l’application garde les scénarios actifs lorsque l’écran Pulsar n’est pas affiché.

| Commande | Rôle |
| --- | --- |
| `pulsar_list` | Lire les scénarios enregistrés dans SQLite |
| `pulsar_save_scenario({ scenario })` | Valider et enregistrer un scénario |
| `pulsar_create_draft({ draft })` | Valider et enregistrer un brouillon Pulsar, toujours désactivé |
| `pulsar_delete_scenario({ id })` | Supprimer un scénario et son journal |
| `pulsar_set_enabled({ id, enabled })` | Activer ou désactiver un scénario |
| `pulsar_simulate({ id })` | Évaluer les conditions sans exécuter d’action |
| `pulsar_run({ id, confirmed })` | Exécuter manuellement après confirmation Windows native; le champ `confirmed` est ignoré |
| `pulsar_logs({ limit })` | Lire au plus 200 journaux |
| `pulsar_fire_event({ name })` | Signaler un événement ORBIT autorisé |
| `pulsar_global_pause_status` / `pulsar_global_pause_set({ paused })` | Consulter ou suspendre globalement les exécutions automatiques |

Crates utilisées: `tauri`, `tauri-plugin-notification`, `rusqlite` avec `bundled`, `serde`,
`serde_json`, `chrono`, `uuid` et `sysinfo` (déjà dépendance Core). L’interface sélectionne les dossiers, fichiers et exécutables `.exe` avec le plugin
Tauri Dialog; les paths sont revalidés par Rust avant chaque action.

`PulsarDraftSpec` contient `name`, `description`, `triggers[]`, `conditions[]`, `actions[]` et
`allowScheduledFileChanges`. L’API native génère l’identifiant et les horodatages puis force
`enabled=false` et `allowScheduledFileChanges=false`, même si l’appel demande le contraire. La
commande de sauvegarde d’un scénario applique la même règle : les valeurs `enabled` et
`allowScheduledFileChanges` reçues du renderer ne sont jamais des autorisations. Une première
migration révoque les anciens grants fichier écrits par les versions précédentes, désarme les
scénarios concernés et efface leur état de déclenchement; l’utilisateur doit les réactiver et
réautoriser leurs actions fichier. Les anciens scénarios stockés avec un seul champ `trigger` sont migrés à la lecture vers `triggers[]`;
le premier déclencheur reste exposé dans `trigger` pour compatibilité. Les scénarios déclenchés
Les brouillons ne s’activent jamais sans action manuelle dans Pulsar.

## Déclencheurs pris en charge

- Manuel, avec confirmation à chaque exécution.
- Démarrage ORBIT, une fois par processus.
- Intervalle de 30 secondes à 24 heures.
- Date/heure ponctuelle.
- Nouveau fichier à la racine d’un dossier choisi.
- Modification d’un fichier choisi.
- Événements `core.alert`, `echo.entry-added` lorsqu’ils sont publiés sur le bus
interne ORBIT.
- Démarrage d’un exécutable `.exe` choisi par l’utilisateur ou fourni par un chemin exact. Pulsar
  compare son chemin canonique aux processus Windows observés par `sysinfo`; il ne lance jamais
  l’application surveillée.
- Délai ponctuel dans un brouillon Pulsar (`after-delay`, 1 seconde à 24 heures), converti en date
  UTC à la création et renvoyé sous forme de `schedulePreview` autoritaire.

Un scénario accepte jusqu’à douze déclencheurs distincts : un seul suffit (OU). Toutes les conditions
doivent être vraies (ET), puis les actions s’exécutent dans l’ordre. Si plusieurs déclencheurs sont
vrais dans le même cycle, Pulsar exécute le scénario une seule fois. Une date/heure ponctuelle est
consommée après sa première tentative; les autres déclencheurs restent armés. Si ORBIT est fermé à
l’heure prévue, le rappel est marqué Ignoré et désarmé au prochain démarrage; il n’y a pas de
rattrapage, donc ORBIT doit rester ouvert pour recevoir un rappel à l’heure.

Le planificateur vérifie les intervalles, changements de fichiers et transitions de processus toutes
les deux secondes. Les
modifications de fichiers sont détectées par comparaison de métadonnées; cela peut manquer deux
changements ramenés aux mêmes valeurs de taille et d’horodatage. La surveillance ne descend pas dans
les sous-dossiers. Le déclencheur « nouveau fichier » ne surveille que le premier niveau du dossier.
Le lancement d’un processus est une transition absent→présent : un programme déjà ouvert au moment
où Pulsar arme la surveillance ne déclenche pas le scénario; une seconde instance du même `.exe`
pendant qu’au moins une instance tourne ne déclenche pas non plus.

## Conditions et actions

Conditions: extension, fragment de nom, taille minimale, existence du chemin, plage horaire locale.
Toutes les conditions doivent être vraies. Sans condition, l’action est éligible dès son déclencheur.

Actions: notification Windows, ouverture de dossier, ouverture de Bloc-notes/Calculatrice/Explorateur,
copie, déplacement, renommage et entrée générique au journal local. Aucun shell, terminal, script ou
chemin d’application libre n’est accessible.

Copie échoue si le nom existe déjà. Déplacement utilise le renommage natif du système et échoue si le
déplacement de volume est impossible; il ne remplace jamais une destination. Le renommage accepte un
nom simple dans le même dossier. Toute action qui écrit des fichiers (copier, déplacer, renommer) en
arrière-plan nécessite une autorisation explicite. Lors de l’activation d’un tel scénario, Rust ouvre
une boîte de confirmation Windows native pour accorder cette permission persistante. Une invocation
manuelle ouvre toujours une boîte de confirmation Windows native; un `confirmed=true` fourni par le
renderer ne l’autorise pas. Désactiver le scénario révoque la permission fichier. Les chemins sont
canoniques et revalidés par Rust avant les actions; les dossiers surveillés sont choisis avec un
sélecteur natif. Les chemins restent soumis aux droits du compte Windows de l’utilisateur.

La simulation vérifie les conditions et décrit les actions, mais ne lance aucune action système. Le
planificateur refuse plus de dix exécutions par scénario et par minute et limite la liste d’actions
par scénario à douze. Les scénarios, états de déclencheur et 500 derniers journaux résident dans
`pulsar.sqlite3` dans le dossier local de données ORBIT. Les journaux contiennent des états et des
résumés génériques, jamais le contenu des fichiers.

## États d’interface

- `loading`: chargement du store natif.
- `unavailable`: mode navigateur; les opérations natives sont désactivées avec explication.
- `empty`: aucun scénario, création guidée et modèles installables; tout nouveau scénario est enregistré désactivé.
- `editing`: propriétés, déclencheurs multiples, conditions et actions modifiables.
- `saving`: boutons d’action désactivés jusqu’au résultat.
- `simulation`: résultat lisible sans effet de bord.
- `error`: erreur affichée à côté du module.
- `history-empty` / `history-populated`: journal des exécutions.
- `enabled` / `disabled`: état persistant de chaque scénario.

Les événements intermodules ne transportent que le nom d’événement autorisé, jamais le contenu Echo,
un prompt, un chemin ou un fichier.

## Limites

- Le sélecteur de modèle ne publie pas encore le prévisualiseur de condition spécifique à un nouveau
  fichier; les conditions de nom, d’extension ou de taille nécessitent un contexte de fichier fourni
  par le déclencheur. La simulation affiche explicitement lorsqu’aucun contexte n’est disponible.
- Les déclencheurs ponctuels se désarment après la première tentative. Une date passée est refusée
  à la création du brouillon et à l’activation.
- L’état d’activation n’est pas une autorisation Windows de fichier: les droits de l’utilisateur
  restent ceux de son compte. Les chemins hors accès utilisateur échouent avec une erreur visible.
- Les notifications Windows de Tauri ne fonctionnent qu’avec une application installée. En mode
  développement, le runtime peut afficher un nom de processus PowerShell.
- Les templates sont préremplis mais restent désactivés jusqu’à validation.
