# oxide-opponent

`oxide-opponent` is the reactive, best-effort opponent every bot seat runs. The
[specification](../docs/oxide-opponent.md) is normative for this crate, and the
[oxide-opponent skill](../.agents/skills/oxide-opponent/SKILL.md) holds its
working rules.

Each decision spends through one running total in a fixed precedence: defense
and other emergencies, then local guards and island settlements, then an
affordable saving target, then the wounded to Repair Bays, then workers, then
lifts, attacks, clearances, strikes and raids, focus fire and scouting, then
production. Missions command only units that exist: production keeps the stocks
and capabilities they draw on, such as scouts, carriers and local defense,
without promising future units to a mission. A producer is ready when its queue
would run out before the next decision, so it queues its next unit as the last
one finishes rather than after it stands empty; every ready producer left free
trains toward the army's needs. Difficulty caps the unit orders one decision
issues; purchases do not count against that allowance.

Public enemy buildings, the Deep Bores every seat knows of, join the seat's
known enemy buildings each decision as memories of unknown condition: built and
at full health. Targets, raids and held frames treat a Deep Bore the seat never
saw like one it remembers, and it is ordered on by its remembered footprint.

## Workers

A seat with no worker alive, carried or queued queues a Harvester even behind
other work, from protected scrap if it must. A live known scrap node's or
wreck's route is the shortest ground route from the seat's start, or on ground
the start does not reach from the nearest of the seat's Foundries there,
followed back until it comes home to one of the seat's Foundries; it is clear
while it crosses neither the reach of known enemy weapons (a site in sight
cannot fire yet) nor the surroundings of an enemy Foundry, known or presumed. A
wreck near a recent allied loss, or whose route passes one, waits until the
warning expires, as the simulation's own salvage does. Each node or wreck with a
clear route belongs to the nearest built Foundry whose ground reaches it, and is
worked when a Harvester hauling from there repays its price within a horizon
that is longer the more the stance invests at home and stretched by greed. A
worked source wants as many Harvesters as free tiles beside it hold and its
remaining scrap or salvage repays, filled to a share that falls from Turtle to
Aggressive and rises with greed, and ready Foundries train workers until every
worked source has its crew; workers and queues count only on the ground they can
serve. Until the army reaches the stance's minimum, workers that would cost more
than the army wait for army production and take only the Foundries it leaves.
Once a Fabricator stands, an Excavator, worth two Harvesters, fills two open
places when the seat can pay for it with scrap to spare, less the greedier it
is.

A harvesting or idle worker away from home runs back beside the nearest Foundry
once known enemy fire reaches it. Paid sites nobody is building get the nearest
free worker. While no armed enemy in sight stands near it, no known enemy weapon
reaches it and the seat has scrap to pay, each damaged building nobody welds yet
that has a free tile beside it gets the nearest free worker, most missing value
first. Wounded armed ground units at home, free or in an attack regrouping
there, come next under the same safety and scrap rules: one worker for so much
missing health among them, less the more the seat leans on support, and at least
one, each sent to the unit missing the most value, sturdiest workers first, so
an Excavator before a Harvester, then idle before harvesting, then nearest. A
worker welding a unit that has left home stops rather than follow it out. Idle
workers go to the reachable worked node or wreck with the most places open; with
none open, to the nearest wreck with a clear route holding more salvage than the
workers on it carry away in a load each, then to the most open worked source
with something left, or with none worked to the nearest node with a clear route,
never one inside the reach of known enemy weapons. A worker harvesting a node or
wreck whose route has turned dangerous is sent elsewhere the same way, or home.
A worker left with nowhere to go first delivers any scrap it carries to a
Foundry on its ground.

## Repair Bays

After defense, free wounded armed units go to a built Repair Bay with no armed
enemy in sight near it and no known enemy fire reaching it: fire against ground
for units that heal on the ground, fire against aircraft for those that fly in,
and both for a Condor. The bot counts each Bay's slots as the simulation fills
them: Bays in building order, each taking up to four of the most wounded units
in its reach that an earlier Bay did not take. A unit missing at least a tenth
of its health that a calm Bay already treats stays there; an idle one goes to
the nearest calm Bay with a free slot, most missing value first, ground units on
the Bay's ground to the open tile beside it nearest them and aircraft over it,
each around the known anti-air on its own way, where an idle Condor sets down on
its own. Missions and welders leave those units alone for the rest of the
decision, and since missions take only units at least half healthy, one badly
hurt on its way is not drawn off before it arrives. The Bay bills its heals as
income arrives, so no scrap is set aside for them.

## Defense

A visible enemy that can hit ground threatens the seat when it stands within a
set gap of one of the seat's buildings, or within its weapon's reach if that is
longer, or when its shells could land where hostile shells are landing near
them. So does a known enemy building, in sight or remembered, whose shells could
land there, while the units that can hit ground the defense holds or could take
outweigh the known defense around the tile beside it nearest the Foundry by half
again; defenders go to that tile. A battery among stronger defenses is left to
production. Threats group by the built Foundry each is nearest, and each group
gets one defend mission, the home Foundry's first. Only threats on or beside
that Foundry's ground count: one across water or a chasm is left to production,
since chasing it would stall every defender. The mission recruits free units
that can hit one of its threats, ground units only from that Foundry's ground,
nearest the Foundry first, until against ground and air attackers alike they are
worth half again what those attackers are. It sends them in one Hunt at the
grounded threat nearest the Foundry, except that artillery is reached rather
than hunted, since a Hunt stops for whatever comes first, such as a spotter
overhead, while the guns shell the defenders from beyond their reach: with a gun
in sight on the Foundry's ground, those that can hit ground attack the nearest
one and are sent again when it falls. Against flyers alone they wait beside the
building nearest the raid, where anti-air reaches them, since the ground under a
flyer may be none they can stand on. It sends them again only when its goal
moves a few tiles on the Foundry's ground or the mission re-engages, and a
defense that stops fighting while focused sends its units back to its goal
rather than after the retreating enemy. With no threat left the mission
recovers, lending its units to any other threatened Foundry, and after a quiet
spell it lets them go where they stand. Once its own Foundries are answered, an
ally's Foundry under ground attack gets a defend mission of its own from the
units left free, which lends them back whenever the seat's own Foundries need
them; an ally's shortfall is never the seat's emergency.

A hostile shell landing near the seat's buildings that no enemy it knows of
could have fired comes from a gun out of sight. Once the built Foundry nearest
the impact has no threat in sight, it gets a defend mission that sends units
that can hit ground, worth half again one of the cheapest guns, toward where the
gun probably stands: from the impact toward the nearest hostile start, as far as
artillery reaches, on the Foundry's ground. They Advance there, so they do not
stop for anything on the way. Once the gun is in sight the ordinary defense
takes over.

A defense that cannot recruit enough makes the decision an emergency: it skips
the saving purchase, trains no Harvesters beyond the one a seat without any
needs, and lets production spend protected scrap. Ready producers the army's
roles leave idle then each train the costliest armed unit the scrap on hand buys
that can reach and hit a Foundry whose defense is short, until what the decision
queued makes up each Foundry's shortfall, so a seat too poor for its line unit
or a gun still fields what it can afford; a ground unit counts only for a
Foundry on its producer's ground. Shelling, from a building or a gun out of
sight, never makes an emergency, since static defenses cannot reach the guns.

## Home reserve

Attacks, lifts, clearances, strikes and raids leave a reserve at home: units
there, meaning aircraft and units on the start's ground, worth enough against
ground and against aircraft for what could come. Against each, the reserve is
nothing while no enemy could reach home that way. Otherwise it is the stance's
floor or its share of the known enemy army that could, whichever is more, less
the built static defenses on the start's ground that cover it and the units
already out defending it; Turtle keeps the most and Aggressive the least. Ground
reaches home from a hostile start connected by ground, or once an enemy carrier
or Airworks is seen; aircraft once enemy armed aircraft or an Airworks is seen.
Offense takes only units beyond the reserve, leaving those nearest home; a lift
leaves its weakest riders. Defense takes every unit regardless.

Every other base, and each Extractor no completed Foundry supports, keeps a
local guard: the stance's share of the known enemies near it, against ground and
against aircraft, less the completed guns whose range reaches it. A quiet base
keeps none. The guard takes the nearest free units that can reach it, never what
the home reserve keeps, and ready producers on its ground train toward what it
still lacks. A defense answering an attack on the base takes its guards along,
and the guard forms again once that defense has recovered.

## Static defense

The seat guards each base, a Foundry it started or founded on an expansion site
with every other building but defenses nearest it, and each Extractor far from
every Foundry on its own. Turrets, Bastions and Flak Turrets go a short gap from
one of those buildings, on the side the threat comes from and in front of the
base's edge, the building furthest toward it: enemies in sight, else enemies it
remembers, else known enemy buildings or the nearest hostile start. Where every
ground way in from a hostile start runs through a cut, a few gates a few units
abreast each, together narrower than their distance from the start and clear of
Extractor frames and expansion sites, beyond the base's buildings and with the
threat beyond it, the guns stand on the home side of each gate instead, holding
the narrowest such cut; no building stands in a gate. Ground threats count only
where ground connects them to the building, except that a seat under the
stance's minimum army also puts up a Turret against enemy buildings and starts
across a chasm, whose units could land. Flak Turrets answer seen or remembered
enemy aircraft or a known enemy Airworks. A gun holds off the army scrap of
Sentinels that would match its damage and health in a fight, at its current
tier, a splash shell counting several hits when the armed enemies behind that
approach gather in clumps, and none of the share of the threat that can hit it
from beyond its reach or from inside its minimum range. Guns hold a threat once
together they hold off its value divided by the margin an attack is assumed to
bring over a defense, the same at every difficulty; the threat is the armed
enemies known behind the approach, on ground connected to the building unless
they could only land, and at least an army at the stance's minimum. Each site is
an investment worth the shortfall it closes at the points of the approach it
covers, from the edge or each gate outward, at most what the gun holds off at
each, per scrap of its price, by the building's value, by how sure the seat is
of the threat, and by personality: fortification for Turrets and Bastions,
fortification and support for Flak Turrets. A seat under the stance's minimum
army with no Turret puts one up before any tech, whatever its fortification: one
gun holds an early rush that a tech building still going up would not. A Bastion
counts only ground the seat's or an ally's buildings see, and is offered only
once the seat owns a Fabricator; the seat buys none for it. When the threat is
only a guess from public facts, equal sites go to the one nearest the building;
otherwise to the one nearest the threat. Sites go only on ground where one of
the seat's workers stands to build them, and a site the simulation refused is
skipped for a while.

Once the opening economy is up, a seat whose army is still under the stance's
minimum weighs a hostile start nearly like seen enemies; otherwise public facts
alone move only very fortified seats. A defense that cannot recruit enough also
buys an emergency Turret, or Flak Turret against aircraft, beside each building,
inside the base or not, whose approach nothing covers when attackers stand near
it, one unfinished at a time beside each.

Arrays watch the far part of those approaches: a site is worth the points out
there that nothing the seat owns sees yet, and its radar lets Bastions fire that
far. A Barricade goes a tile in front of a Turret or Bastion that has none, on
the side of the threat; like a packed building, it is refused at purchase when
it would close a way out. Guns, Arrays, Barricades and Repair Bays stand off the
lanes of the base's layout. Once a Fabricator stands, Scuttle Charges mine the
straight way in to each base and Extractor on its own: the lanes within a band
about a blast wide, filled from a few tiles beyond the edge toward the threat,
or across the gates of the cut it holds, each by its width and the share of its
way on the guns leave open. On ground no Foundry lays out they may go anywhere.
A buried charge blocks nothing and a blast never sets off the seat's own
charges, so they stand side by side. A field holds enough charges to deal the
health of the threat along the way divided by that margin, in the share of the
approach the guns leave open, one body to a blast, the threat counting at least
an army of Sentinels at the stance's minimum. Fortification weighs Barricades;
fortification and guile weigh Arrays and charges.

A Repair Bay goes up beside a guarded base's building where its aura reaches the
most missing value among the seat's resting units missing at least a tenth of
their health and its damaged buildings that no Repair Bay treats yet, weighted
by support. A Bay, standing or planned, treats the buildings it reaches and as
many units in reach as its slots hold after earlier Bays took theirs, so wounded
a full Bay cannot take can justify another beside it.

The seat expects enemy Spires before it sees one. An enemy holding a Deep Bore,
which every seat sees, or a seen Crucible counts as one possible Spire, and
earns a Lattice over the most valuable base only; an enemy holding both earns
Lattice cover over every base, as a known Spire does. Known enemy Spires, seen,
remembered, or given away by a tracked warhead's line, call for interceptors in
proportion. A base counts as covered by the own Lattices, built or not, whose
ring holds its costliest point, where one warhead would take the most; a site is
worth the bases it would cover, weighted by fortification, and by half when the
threat is only one half of a Spire's tech. Among equal sites the one nearest
home wins, so the battery stands behind what it guards. Arrays then also watch
where the line from the nearest known enemy Spire, or failing one the nearest
known Spire tech, crosses into the ring of a Lattice guarding the base, just
outside, at and just inside it, so radar holds a warhead long enough for the
Lattice to stop it.

Defenses upgrade in place (Heavy Turret, Bulwark, Burst Flak, Citadel, Deep
Array) once the next tier's prerequisite stands, worth how far the approach they
cover still falls short of holding, counting for a Citadel only ground something
sees, or for an Array the far points it watches, by how sure the seat is of its
threat and weighted by fortification and greed, and never while an enemy in
sight could hit it, from its own reach or the defense's, while it is down at a
fifth of its health.

## Attack

Strikes and raids run side by side: each decision advances every mission of a
kind, oldest first, then forms another while the free units beyond the home
reserve meet the need of the best target no mission of that kind holds, each
taking only the force its own target needs. An attack and a lift each run one at
a time and take every free unit they can use: one army that wins what it meets
beats several that each meet the enemy alone. Missions of different kinds may
still go after one target.

An attack forms, when none is under way, from the free, healthy line, siege,
anti-air and anti-heavy units beyond the home reserve, and takes all of them,
when those that can hit ground are worth at least the stance's minimum army and
outweigh the known army it would meet by a margin set by difficulty; anti-air
units recruited along the way escort the army but do not count toward that. The
army it would meet is every armed enemy the seat remembers that hits ground, on
the target's ground or in the air, and the known enemy buildings guarding the
target. Remembered units count by confidence, except that a ground unit on
ground the seat's own does not reach counts in full; buildings that fire on
ground count at their price with every upgrade they reached, discounted by
missing health, and a building's weapons reach as far as its current tier's do.
The target is the most valuable known enemy building for its distance by ground,
or a hostile start when none is known; a better one replaces it only while the
army gathers or recovers, and only when clearly better. The army gathers at a
rally near home toward the target's owner, where free units join it while it
gathers or recovers, travels, and fights; recruits still on their way go along
once the time to regroup is up. It withdraws to the rally when the enemies it
knows of around it, remembered or seen, outweigh what it has left, and recovers
there to go again or disband. Having taken its target it pushes on to the next
only while strong enough for it. A target it withdrew from, could not reach,
lost its army to, or stood idle beside is skipped for a while. With several
enemies, attacks and strikes go after one rival's buildings first: the enemy
pressing the seat hardest, then the nearest, less the army it shows, with guile
favoring a small economy and a bonus for the owner of the oldest attack or
strike's target so the seat does not flip between enemies. The rival is chosen
once per decision, after defense.

Free, healthy point defenders on the target's ground go along with an attack
when it forms and join it wherever free units do. They never count toward its
strength, its readiness, its arrival or contact, or the opposition it meets.
While it travels or fights, a defender farther than half its reach from the
fighting member nearest the army's middle runs back to that member's tile. An
attack left with no fighting member sends its defenders home and ends.

Badly wounded members leave between fights and run to the rally, and a defense
may take the attack's units in any phase but a fight. While a free army that
could attack does not, or every producer sits idle, the margin for the next
attack falls step by step toward even.

Difficulty changes how well a seat attacks, never what it may do. Veteran and
Prime count the known defense around a target like more distance, so they go
after the weakest valuable target; send units freed while an attack travels or
fights straight to its target; keep a fight they are still winning going past
its time; and withdraw only once outweighed by a quarter. Standard and Scrapheap
leave those units at home until the attack regroups, end a fight on time and
withdraw at even odds. Scrapheap also attacks piecemeal, several attacks side by
side each taking only what its own target needs; goes for a Foundry whenever it
knows one, however well guarded; and misjudges the enemy's strength by up to two
fifths either way, a misjudgment drawn from its personality seed that holds for
600 ticks, both when setting out and when deciding to pull back.

An engaged mission focuses its fire: when every member that can hit an enemy
near it already reaches that enemy in a straight line that terrain does not
stop, and every member on the ground stands on that enemy's ground, they shoot
one such enemy together and keep that focus while it stays in reach. Veteran and
Prime pick the enemy with the least health, Standard the one whose health bar
shows the lowest quarter, and Scrapheap the one nearest the mission's goal
whatever its wounds; ties go to the nearest. Nobody chases a focus or seeks a
way round to it, and a member out of reach leaves the mission's fire unfocused.
A member whose kind picks its own targets heaviest first, such as a Piledriver,
keeps its own aim and takes no part in the focus: the weakest enemy would waste
its single heavy shot.

A site on ground no own worker stands on needs a settlement: one builder lifted
from the nearest Foundry whose ground a worker stands on, with ground escorts
aboard and air escorts flying beside while known enemies stand near the site; a
gunship among its carriers flies on without stopping for what its guns reach.
The expansion's next purchase is a Harvester while no Foundry's ground has a
worker, then an Airworks, then a Skyhook, until carriers can take the builder
and its ground escort. The settlement only delivers: once the builder lands it
goes back to ordinary work, and the expansion is an ordinary build that worker
raises. The escort holds the landing until the building is placed, or until no
known enemy stands near, then joins the site's local guard, which it was sized
to be. A flight that runs late turns home; a builder stranded on the island
builds there on the next attempt. Settlements and combat lifts run side by side
with separate members. Settlements move before guards each decision, so a
released escort is guarded at once, and new ones recruit after guards.

## Lift

When the seat knows of enemy buildings or hostile starts and ground reaches none
of them, and the seat has at least the stance's minimum army to carry, at home
beyond the home reserve or free of other missions on other ground it holds where
no target stands, it needs lift. The Airworks then scores higher while the seat
has none, and production keeps enough carriers, alive, queued or being
assembled, to carry every free rider on the ground a lift would load at once,
packed as a lift would pack them into the largest holds first, training Skyhooks
for the riders left over. While a ready Airworks waits for the scrap to train
one and some of those riders have no room, other production waits too, unless a
defense is short. Carriers and scouts train even at an Airworks held for a unit
the seat saves for.

One lift runs at a time. It loads on one ground: home, or other ground the
seat's units stand on where no target stands, an army trained at an expansion or
set down by an earlier lift, whichever ground's riders bring the most, home on a
tie. It forms only from carriers and passengers that exist: free, healthy line,
siege and anti-heavy units on that ground, beyond the home reserve at home,
packed most value per transport slot first into free, idle, empty carriers,
gunships first and then the nearest that ground first, wherever they hover but
over ground there no rider could reach. It needs those that can hit ground, with
its gunships' own guns, to be worth the stance's minimum and to outweigh by the
attack margin the known army it would meet, counted as for an attack with the
guns guarding both its landing and its target, since riders set down out of
range still walk into the target's. At Veteran and Prime, the bombers out
clearing the anti-air around its target count toward that need. Except at
Scrapheap, a lift also waits for a recent look at its target: the scouting point
nearest the target on its island must have been in sight recently enough that
the enemy units seen there are still remembered. Every free carrier and rider is
loaded as the decision's orders allow, and more board on later decisions while
boarding lasts; carriers elsewhere fly to open ground beside the riders, and an
empty carrier of the lift takes more once none of its riders is still walking.
The target is the most valuable known enemy building, or hostile start not seen
cleared, for its distance that no ground route reaches and that has a landing:
open ground on the target's island near it, as far as the seat knows, out of
reach of known fire where any is, and then a few tiles from it, so riders set
down out of range walk in, where every tile unloading could set a rider on
belongs to that island.

A free carrier hovering where no rider could reach it, such as over the Airworks
that trained it, first moves to open ground, at home or on the ground the lift
loads at. Riders walk to their carriers and board. Once none is still walking
and no more were sent, or after a while, the loaded carriers leave together in
one order if what is aboard, with any bomber cover the seat counts, meets the
landing's need as now known; a rider that stopped short is not sent again. A
lift with a gunship sends only its gunships then, so known anti-air meets them
first, and its other loaded carriers follow once a gunship is over the landing,
or set their riders down where they wait if every gunship is lost. Otherwise the
lift sets everyone down and disbands, and riders still walking are stopped and
let go. Carriers fly straight to the landing, or around known anti-air through a
via-point when the straight line crosses it. Each sets its riders down at the
landing once there, as the decision's orders allow, and emptied carriers fly
home together the same way. An emptied gunship instead stays over the landing
and hunts beside the riders, flying home when their fight ends, or alone once
wounded. Landed riders hunt the target and, once no one is aboard and every
emptied carrier is on its way home, fight on to the next target on the same
island. Nothing brings them home. A carrier that comes home still loaded sets
its riders down and lets them go. A target the lift lost its units to, or stood
idle beside, is skipped for a while. A lift never grows past the mission member
cap, and a defense may take its units only while they board, never its carriers.

## Clearances

While ground reaches no enemy, the seat's free, healthy splash bombers, its
Condors, beyond the home reserve go ahead of its lifts, at the remembered enemy
anti-air ground units and known built anti-air buildings, such as Flak Turrets,
on ground the seat cannot walk to around the building a lift goes after, or the
one the next lift would. A clearance forms when those bombers are worth the
stance's minimum and together outweigh, by the attack margin, all the known
anti-air reaching over that building or any of that anti-air, so they go in
together rather than one at a time; it takes every one of them. They gather
beside home on the side facing their aim, go first at the anti-air whose known
cover they outweigh most easily, sweep it and move on to the next, and once none
is left sweep the building's surroundings, holding the defenders' attention
while the carriers land. They fly around known anti-air when the straight line
crosses it, and withdraw once the known anti-air reaching them outweighs them.
Ordinary strikes leave splash bombers to clearances while ground reaches no
enemy.

## Strikes

Free, healthy Buzzards, and Condors while ground reaches an enemy, beyond the
home reserve, strike the most valuable known enemy building or hostile start for
its distance, whether or not ground reaches it, when those that can hit ground
are worth the stance's minimum and outweigh the known anti-air reaching over the
target by the attack margin. They gather beside home on the side facing the
target, fly at it around known anti-air, and fight; having taken it they go on
to the next target they can, and otherwise fly home. They withdraw once the
known anti-air reaching them outweighs them. A target a strike withdrew from,
lost its aircraft to, or stood idle beside is skipped for a while. A defense may
take a strike's aircraft while they gather or withdraw.

## Warheads

The seat values a warhead on each footprint tile of every known hostile
building, seen, remembered or public, and on a hostile start it has never seen
as the Foundry there: the scrap paid for each building times the share of its
known health the simulation's own falloff and resistance would take there, after
what the team's warheads already flying will take. Known hostile Lattices whose
ring holds the tile count their loaded interceptors when seen and a full
magazine when remembered. A volley is worth firing when the warheads that get
past those interceptors take at least the price of every round it spends.

Loaded Spires with no strike pending launch at such a volley: Veteran and Prime
at the one that takes the most beyond its price, Scrapheap and Standard at the
worthy aim nearest home, each with the fewest Spires that get through, the
Spires nearest the aim firing. Launches are unit orders and count against the
decision's allowance; a volley the allowance cannot hold whole waits. Otherwise
loaded Spires hold.

Once a Crucible stands and a known strike is worth a round, the seat wants a
Spire, upgrading its own supported Extractor nearest home into a Deep Bore first
when it lacks one; a round for each empty Spire once, with the others stocked,
it could land one where it is worth it; and another Spire when every Spire is
stocked or going up and known interceptors over every worthy aim match the
Spires it has. Siege weighs them, each more the more its best strike takes, and
income scales them as it does the Crucible. Rounds are saved for like any
investment and ordered once the bank covers one. Autofill stays as each kind
starts: Lattices reload on their own and Spires do not.

## Raids

While no defense is under way, free, idle and healthy raiders of one kind beyond
the home reserve set out on a raid, those nearest the target until they are
worth what it needs. Scuttlers and ground-attack aircraft too few for a strike
go after the enemy Extractor or Foundry whose known defense they outweigh by the
attack margin, least defended and then nearest first, Scuttlers only where no
known, built enemy building's ground fire reaches and the known guard is light
for the stance, the aircraft flying around known anti-air. Sappers go after the
most valuable known enemy building for its distance with little known defense
that they are enough to blow up. Scuttlers and aircraft turn back once one is
badly hurt, once the known fire reaching them outweighs them, or once they have
been at the target a while; Sappers ignore wounds, and fire once at the target.
Raids skip a raided target for a while, which spaces them out, while attacks,
lifts and strikes may still go after it.

Once its income reaches a level that falls with guile, the seat keeps the
Scuttlers, alive or queued and out on no other mission, that a raid on the least
defended known harvest line no enemy gun guards and only a light guard holds
needs. Once it has scrap to spare, less the more it leans on siege, it keeps a
Sapper for each known enemy defense, in sight or remembered, that can hit ground
around the targets of attacks under way and of the next attack. An attack on a
target with such defenses takes a free Sapper along for each while it gathers or
recovers, and once it fights sends each at the nearest such defense on its
ground, leaving them to it when the rest of the army moves on.

## Breachers

Once a Crucible stands, the seat wants a Maw cradle while its line, siege and
anti-heavy army, with the Maws it fields and the cradles it raises at a Maw's
price, falls short of what an attack on the known enemy guns its ground reaches
would need, by the attack margin; siege weighs it and income scales it. While
the seat neither fields a Maw nor raises a cradle, an attack, lift or strike
lately given up on ground the seat reaches is direct evidence the army cannot
break in: the cradle is then wanted at the scale of the seat's upgrades. The
cradle's next step reaches its Deep Bore first. It stands in a whole block
beside a Foundry, and only where the Maw's hull could drive out past the seat's
furthest building. The seat tops its crew up with one order to the free workers
on its ground, the fastest builders first and never a node's last Harvester.

A breacher's value counts a tenth against machines, its cutter's share, and in
full against buildings. Every free breacher that can get there joins an attack
as it launches or regroups; what breachers bring against buildings answers the
guarding buildings in the attack's need and in the opposition its fight weighs.
In a fight an idle breacher cuts the nearest defense, staying at it while the
rest of the army moves on, and nothing at home mends it, so wounds never send it
back. Between fights a free, wounded Maw drives to the nearest place its whole
hull can reach through known ground where its kiln would reach wreck its
harvesters could safely salvage, one holding enough to mend it first, and sits
out missions there while it is calm and its kiln has wreck to eat; harvesters
leave that wreck to it, on its way and once there. A known enemy cradle counts
as the machine it will stand up, a Maw's as a heavy machine and a Roc's as air,
and the machine that stands up from it is remembered where it stood up, even out
of sight, since every seat learns of a public cradle's rollout.

## Gunships

Once a Crucible stands, while its ground reaches no enemy target, the seat wants
a Roc cradle while the known anti-air around its next lift's target, by the
attack margin, is more than the Rocs it fields and the cradles it raises can
take at a Roc's price; air weighs it and income scales it. While the seat
neither fields a Roc nor raises a cradle, a lift or strike lately given up on
ground the seat cannot reach is direct evidence its lifts do not get past: the
cradle is then wanted at the scale of the seat's upgrades. The cradle's next
steps reach its Deep Bore and Airworks first, and it stands and is crewed as a
Maw cradle is. A Roc leads the seat's lifts, as described above; while ground
reaches the enemy, an empty one joins the next attack with the army instead.
Local guards never take a carrier.

## Scouting

The seat keeps its hostile starts and expansion sites as scouting points and
remembers when it last saw each. Each point unseen for a while draws its own
scout, first the one whose time unseen, weighted by its value, is greatest:
hostile starts weigh most, then sites nearer an enemy than home. A scout moves
on to the next point no other scout holds when it sees its own. Scouts are free
Surveyors on home ground, which take the points they can walk to first, and
Kestrels, which take the rest. For each stale point no scout could take, the
seat trains one that could reach it: its air scout once it has an Airworks, a
Surveyor before. It trains none while no mission could take another. A Surveyor
is sent with the Scout order, so it keeps out of known reach on its own; an air
scout flies around known anti-air when the straight line crosses it. A point
whose scout was lost on the way, stopped short of it, or found no safe view
counts as seen, so the next scout waits until it is stale again.

## Army composition

The seat remembers every enemy unit it has seen for 600 ticks, trusting them
less as they age and forgetting one when its last spot is in sight and empty. An
enemy ground unit seen on ground the seat's own does not reach is remembered for
3,600 ticks instead, and counts in full for attacks, lifts and known fire until
then, since it cannot leave there without carriers. Enemy buildings need no
memory of its own: the observation keeps their ghosts.

From that knowledge and its own army, alive, carried and queued, it sets a
deficit for each role: line fighters against the enemy's ground army or in
proportion to its own, siege for known enemy defenses and, unless the army goes
only by lift, for a share of its own army behind that line that the stance sets
and siege preference moves (Aggressive seats, which attack early with small
armies, keep the least), anti-air against the enemy air it has seen (a seen
enemy Airworks counts as air), anti-heavy units against the enemy heavies it has
seen, air strikes by preference once it has an Airworks, and point defenders
against the enemy shellfire it knows of while its ground army can walk to an
enemy. That last counts whole defenders: each known gun that fires shells,
rockets or bombs adds the shots it fires in one defender's recharge, weighed by
its shot's damage against the heaviest projectile's, so a few heavy launchers
call for one and a stray light gun does not. Personality's support preference
caps the defenders at a share of the armed army, to the nearest whole defender,
and they never add to that army's size. The deficit counts the worth of the
known guns whose fire no defender answers yet, as anti-air's counts the enemy
air, so a screen against costly launchers can come before a line deficit of the
same worth. A point defender has no cheaper stand-in, so the seat saves for one
as it does for a premium unit. A heavy is an armed ground machine whose full
health takes at least two of an anti-heavy unit's single shots whole; once the
seat can train anti-heavy units they answer the heavies and line units the rest
of the enemy's ground army, and until then line units answer all of it.
Anti-heavy units are bought only for that need, never by producers left with
nothing wanted. Ground units count only while they can reach an enemy, by ground
or by lift once an Airworks stands; before then, known enemy ground units on the
seat's own ground still call for line units. While ground reaches no enemy, air
strikes are wanted with or without an Airworks, at least what a strike needs
against the easiest known target the seat has not given up on, or what bombers
need to clear the anti-air around the next lift's target, whichever is more. The
most wanted role goes first to the nearest ready producer that can afford a unit
for it, then the next; ready producers left with nothing wanted train, while
ground units can reach an enemy, for whichever of line and siege they serve is
furthest below its share, line alone when the army goes only by lift, unless
another producer serves a wanted role: the scrap then waits for that producer.
While the seat saves for a unit, the producers that train it wait for it and no
other producer trains a cheaper unit of its role. Each chooses the unit it can
afford now by coarse suitability: reach against the enemy's usual reach, what
its role is for at the price (firepower for siege and anti-heavy, durability for
the rest), covering both enemy domains, splash against clustered enemies,
anti-air that flies in proportion to the known enemy air that fires only at the
ground, affordability at the seat's income, variety (a kind that already makes
up most of its role counts for less, so every kind of a role gets its turn), and
personality. While ground reaches no enemy, a strike aircraft's splash is worth
instead the targets one blast takes in the densest known clump of enemy ground
units; the seat then saves for a splash bomber the more, within a cap, the more
targets its blast takes than the cheaper strike aircraft, and pulls toward the
tech building such a bomber requires. Scouts, carriers, Scuttlers and Sappers
are bought only as the sections above describe. A role it needs but cannot train
at all adds to the investment score of the cheapest building that would let it,
and a role it can train adds to a producer it lacks whose unit would suit it
better, as much as saving for that unit is worth.

## Investments and saving

Each decision scores its investments: a first Fabricator, Airworks and Crucible,
another Fabricator, Airworks, Crucible or home Foundry when every one of that
kind is working and either a unit the seat has saved enough for waits on it, or
an army role it trains is still wanted and the income the working producers
leave unspent could keep one more as busy (carriers, Scuttlers and Sappers never
call for one), an expansion Foundry at a scrap field away from every start, an
Extractor on a free frame, more Reclaimers (worth more with no expansion left,
and each one owned weighing less against the next as the scrap around home runs
out), static defenses, Repair Bays, upgrades, the unit a wanted role suits best
when it costs more than the scrap on hand and the role has a cheaper unit
production would buy instead, worth more the more of two of it the role lacks,
and Spires, their rounds and Lattices (see Warheads and Static defense).
Saturated harvesting, time, income, home depletion, army needs, a needed lift
and personality set the scores.

An expansion site's value weighs the scrap it still holds and its free frames,
up with greed, against its ground distance from home, how much nearer a hostile
start it lies, and the danger the seat knows around it, down with greed; home
depletion raises every expansion. Disconnected sites use flight distance and
include frame-only fields. Sites near another seat's building or its own
Foundry, or failed at every anchor are skipped. An Extractor is worth more
beside one of the seat's Foundries and less where it has seen danger, and none
is bought on a frame while an armed enemy in sight stands near it. Once a
Crucible stands, the supported Extractor nearest home is offered a Deep Bore
upgrade, which needs that Foundry support to start, and an expansion site is
also worth a frame for each own remote Extractor a Foundry there would support.
A Deep Bore is guarded as worth more than a Crucible. The seat saves for one
target at a time. It starts saving only for an investment that scores well and
could stand somewhere it knows of or could look, keeps it while it still scores,
and switches only for one that scores clearly higher. Prerequisites come first:
saving for Airworks buys a Fabricator, and one needing an upgraded tier, such as
a Spire's Deep Bore, upgrades the seat's own building one rung short, nearest
home and with the support its upgrade needs, or waits while that upgrade runs.

When it adopts a target a share of its bank is protected from ordinary spending,
and while it saves a share of its estimated income is added, up to the next
purchase's price. Stance and greed set the share, and armed enemy units in sight
near the base lower it. Income is estimated from the bank's change between
decisions plus the seat's own spending. Once the whole uncommitted bank covers
the next purchase, the seat places it with the nearest free worker on the first
spot its knowledge allows (an expansion site's anchors, a frame, a defense's
chosen site, or otherwise a spot beside one of its Foundries), upgrades the
building, or trains the unit at the nearest ready producer. Every Foundry lays
out the ground around it in blocks with one-tile lanes between them; its own
block is the Foundry and the ring around it, and every other block holds four
two-by-two slots, each touching two lanes. Another Foundry takes the centre of
an empty block, a two-by-two building a slot, and a smaller one a slot's tiles
beside a lane, so buildings pack side by side and every one has a lane beside
it. Once those run out on a ground, any place off the lanes follows, so cramped
ground still takes whatever fits. Spots are open ground clear of frames, the
seat's gates and starting scrap, taken nearest first: every Foundry's spots at
one gap, the start's first, before any at the next, out to the edge of its
ground, each in the layout of the Foundry nearest it, and while any Foundry
stands on ground one of the seat's workers stands on, only beside those. A spot
that would leave the building, a site still going up or a producer without a way
out, seal off a pocket of open ground, or cut the seat's Foundries off from a
home scrap node or a hostile start on their ground, is skipped for a while. So
is a spot for anything but a defense where a gun the seat knows of or an armed
enemy it remembers could hit it, or where it lately took damage. If the first
spot it could use is unexplored, the worker walks toward it instead; with no
spot left to place or explore, nothing is protected, so a target that cannot
stand anywhere never starves production. A purchase missing from the world at
the next decision was rejected, cancelled or refunded: the seat keeps the
target, protects its full price again, and skips that spot for a while.

Placement is checked against the fog-honest observation only: completed
prerequisites, explored ground, frames, known rock and scrap, known buildings, a
one-tile gap to the seat's own buildings, or for a building packed into a block
only to its Foundries' rings (Extractors sit where the map put their frames, and
the seat's own buried charges keep nothing away), visible hostile ground units,
its own claims, and an open tile beside the footprint. Ground explored but out
of sight is claimed as a provisional scaffold. Hidden units and unseen buildings
never change the verdict; the simulation re-checks on arrival.

## Boundary

The crate depends on `oxide-sim` and `chassis`. It reads only its seat's
fog-honest `ObservationData` and its own order events, and emits ordinary
`PlayerCommand`s. Its seats also share one immutable `MapModel`, built once per
match from the scenario's public map: ground components, the authored starts and
teams, ground distance from every start, where building spots may go, home
scrap, expansion sites, and the cuts and gates on each seat's ways in. Distances
between buildings and units are measured between whole footprints, so they stay
equal for mirrored seats. It decides on its difficulty's interval and stays
silent once the match is decided, after its seat surrenders, or while it has no
built Foundry. Equal-distance choices are broken in a frame anchored on the
seat's authored start, so mirrored seats make mirrored choices.

## Own events

Each decision also receives the seat's own `OrderStalled` and `CommandRejected`
events since its previous decision, and every other seat's `UnitAssembled`,
oldest first. `OwnEvents` holds them: the host keeps one per seat beside the
controller and calls `record` after every tick, and a decision takes them all. A
repeat of a unit's stall for the same reason, or of a rejection for the same
reason, replaces the earlier one in its place; the buffer is bounded and drops
the oldest first. A tick without a decision leaves the buffer untouched. A
rejection holds back that decision's income sample. A unit whose order stalled
for want of a route or a firing position sits out orders, and joins no new
mission or work, until it moves off where it stopped or a while passes. The
trace reports the events.

## Selection

Every configured bot seat runs this controller. `oxide-kit` hosts it, so live,
headless, saved and recovered sessions build it from the same scenario data.

## Checkpoint and trace

`Checkpoint` holds the seat and what carries between its decisions: remembered
enemy units, footprints it recently failed to claim, enemy buildings it recently
gave up attacking, lifting to or striking, or raided, its own units sitting out
orders after a stall, its income sample, and its saving target with the
protected amount and any purchase awaiting confirmation (a round counts once its
Spire shows it ordered or loaded), its missions with their phases, members,
goals and focus, since when a free army that could attack has not, and when it
last saw each scouting point. The host saves the seat's `OwnEvents` beside it.
Restoring it checks that the seat is a configured `oxide-opponent` bot in the
bound scenario and world and that nothing it remembers is from a later tick or
off the map, rebuilds the profile and decision interval from the scenario, and
takes the map model built from it. A saved buffer over the cap does not load.

`Opponent::act_traced` returns a `Trace` of the decision's tick, seat, bank,
received own events, committed spending, purchases, unit-order count, allowance,
saving target with its next purchase, protected scrap, and missions. Traces are
diagnostics only. `Opponent::protected_scrap` and `Opponent::missions` report
the protected amount and each mission's phase, when it began and the timeout it
should end within to hosts. `Opponent::launches` reports the attacks the last
decision launched, each with the known defense, margin, need and strength sent
it was judged by and the units sent; it is never saved and no decision reads it.
