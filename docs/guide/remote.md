# Remote

Remote lets you operate your station when you are not sitting at it: from a web browser
anywhere, or from Nexus on a second Windows PC, at home on the same network or away over the
internet. The PC at the shack keeps the radio and stays in charge. What you click, type and
say goes to the station, and the station sends back its screen, its receive audio and what it
did. Your log, settings and passwords never leave the shack PC.

Nexus Remote is a **Beta**. The Settings card says so beside its switch, and access could be
revoked at any time while it is.

## Before you start

- **Nexus on the shack PC, set up and working on the air** at the shack. Remote adds no
  radio setup of its own: [Settings ▸ Radio ▸ Rig & CAT](settings-reference.md#rig--cat).
- **A Nexus Remote account** for the browser and internet roads. You make one the first
  time you sign in, with an email address (a code by email, or a password) or with Google.
  Using Remote on the shack's own network with a code needs no account at all.
- **For the second PC: Windows, and the same Nexus version as the shack.** On Linux and
  macOS, use Remote in a browser for now.

## The tour

### Three ways in

| Way in | Where you are | What you get | Needs |
|---|---|---|---|
| **Browser stream** | Anywhere with a desktop web browser | The shack's Nexus window as a picture you operate, its receive audio with **Listen**, and your microphone in Phone | A Nexus Remote account; the shack's **Stream this station from my browser** on (Windows shacks only for now) |
| **Second PC on your network** | Another Windows PC on the shack's network | The full Nexus screens for the shack's radio, its receive audio, your microphone, Phone PTT, CW from the keyboard and macros, FT8/FT4, JS8, APRS, SSTV, Chat, Tune and ATU | **Remote over this network** on at the shack; nothing on the internet |
| **Second PC over the internet** | A Windows PC anywhere | The same as on your network, through the Nexus Remote service | A Nexus Remote account, and the PC approved at the shack |

### At the shack: Settings ▸ Remote ▸ Remote access

Everything about who may use the station is set here, at the station, and nowhere else. The
card holds:

- **Turn on Remote** — opens an outbound encrypted connection to the Nexus Remote service. No
  port forwarding. If Remote was on when Nexus closed, it comes back on by itself the next time
  Nexus starts.
- **Sign in to Nexus Remote** — links this station to your account, with no code. Each of
  your other devices is then added when you sign in on it. **Pair a station** is the older way,
  with a code; it still works.
- **This station's key** — eight groups of four characters. Every browser and second PC shows
  the key it was given for this station, and you compare the two before trusting it. See
  [Check the station's key](#check-the-stations-key).
- **Stream this station from my browser** — off by default. With it on, an approved browser
  that holds control sees this window and operates it.
- **Remote over this network** — off by default. Lets Nexus on another PC on the same
  network operate the station with no internet at all. **Pair a computer** shows the code a
  PC on the network types to pair.
- **People** — invite someone by the address they sign in with, to **Operate** or to
  **Listen**. Someone invited to operate transmits with your station, under your callsign, and
  you stay responsible for it as the licensee.
- **Devices** — one list of every computer, phone and browser that can reach the station: yours
  first, then each person you invited. Each row says how it connects (**Home network**,
  **Internet**, **Browser**) and where it stands (live now, waiting for you, held from
  transmitting, allowed to transmit), with **Approve** for a new device, **Allow to transmit**
  and **Remove**. Keys, ids and dates fold under **Details**.
- **End remote control** — turns Remote off (it stays off after a restart), disconnects every
  browser and every computer, and turns off Remote over this network.

The field-by-field reference is [Settings ▸ Remote access](settings-reference.md#remote-access).

### On the second PC: Stations on this network…

On the second PC, **Stations on this network…** opens a window listing the stations this
PC can operate. Each row has **Operate from this computer**, the station's key, and when the
station was added by signing in, the date by which to sign in again. **Sign in…** adds a station
with your account; **Pair with a code instead…** adds one with a code from the shack.

While the second PC drives the station, a banner across the top says which station, how the
link stands (connecting, live, not live, or another device in control) and offers **Leave**,
which starts Nexus again as this PC's own station.

### Who can transmit

Nexus at the shack decides, and nothing set from afar changes it.

- **A computer on the shack's own network**, paired there with a code or added by signing in on
  that network, transmits at once. It is never held.
- **A browser or PC that signs in over the internet** can listen and take control at once, but
  cannot transmit until you approve it at the shack, under Remote access, with its key.
- **A browser or PC you approved at the shack** transmits from anywhere. Each one is approved on
  its own.
- Nexus does not email you when something signs in yet. Until it does, a computer that signs in
  stays unable to transmit until you approve it.

### What stops a transmission

Stop always wins, from either end:

- **Stop TX** on the page or the second PC, and **Esc** over the stream's picture.
- The second PC's Stop has a road of its own, so it still reaches the station when the
  command link is slow.
- If the browser's connection drops, the station stops transmitting within five seconds; if
  its picture freezes, within seven. If the second PC or its network drops, the shack unkeys
  within about 5 s.
- Every Stop at the shack itself works as always.

## Core workflows

### Set up the shack

1. At the shack, open **Settings ▸ Remote ▸ Remote access**.
2. Press **Sign in to Nexus Remote**. Your web browser opens on the Nexus Remote sign-in.
3. Sign in, or make an account. The card then shows the station as linked to your account.
4. Press **Turn on Remote**. The card shows the station connected to the Remote service, and
   **This station's key**.
5. If you will use a browser: turn on **Stream this station from my browser**.
6. If you will use a second PC on your own network: turn on **Remote over this network**. The
   card says which address the station listens on, or what stands in its way (a network Windows
   calls Public, say). When Windows asks whether Nexus may use networks, allow **Private
   networks** only.
7. Optional: say yes when Nexus offers to start when you sign in to this PC, so Remote comes
   back after the PC itself restarts.

Expected result: the card shows Remote on, the station's key, and the switches you chose.

### Stream the station in a browser

1. On the computer you are at, open the Nexus Remote page and sign in with the same account.
   Your station is listed with its key.
2. Compare the station's key on the page with **This station's key** at the shack, group by
   group, the first time you see it.
3. Press **Stream**. The first time, the shack asks **Let Chrome on Windows stream this
   station?** (or whichever browser it is), with the browser's key.
4. At the shack, press **Approve browser** only if the key matches the one the browser shows.
   You approve each browser once; it stays approved until you remove it.
5. The stream opens with the shack's Nexus window. Click and type over it as you would at the
   shack.
6. Press **Listen** to hear the receive audio; **Volume** beside it sets how loud.
7. In Phone, turn on the microphone and hold PTT to talk: see
   [Talk from Remote](phone.md#talk-from-remote).

When it is over, press **Stop TX** if anything is on the air, then close the page.

### Add a second PC by signing in

1. At the shack, sign in to Nexus Remote first ([Set up the shack](#set-up-the-shack)) and turn on
   **Remote over this network**.
2. On the second PC, open **Stations on this network…** and press **Sign in…**.
3. Sign in in the web browser that opens, with the account the station is linked to, or one its
   owner invited to operate it.
4. Back in the window, the station is found on the network by name and added, with no code. If
   it is not found, type the address the shack's card shows, or press **Not on this network** to
   add it over the internet only.
5. For the internet road: at the shack, approve the PC under Remote access. Nexus asks there with
   the PC's key; approve only if it matches the key the PC shows. On your own network the PC needs
   no approval.

Expected result: the station's row offers **Operate from this computer**. A PC added by signing
in stays on the station's network for 30 days from that sign-in. In the last week the row says
by what day, and offers **Sign in again**.

### Add a second PC with a code

Use this when the shack is not signed in to Nexus Remote, or the service cannot be reached.

1. At the shack, with **Remote over this network** on, press **Pair a computer**. The card shows a
   code for ten minutes. Whoever types it in that time is paired, so keep it to yourself.
2. On the second PC, open **Stations on this network…** and choose **Pair with a code instead…**.
3. Pick the station from the list (it shows as "Nexus" and the first characters of its key), or
   type the address the shack shows.
4. Type the code. Capitals and spaces do not matter.

Expected result: the station is on this PC until you remove it at the shack, with no 30 days to
count. Up to eight computers can be paired.

### Operate from the second PC

1. In **Stations on this network…**, press **Operate from this computer** on the station's row.
2. Nexus starts again driving the station. The banner reads **Operating the station at …
   over this network**, or **over the internet (Beta)** when the station is not on your network.
3. Operate as you would at the shack: the screens, audio, microphone and Stop are the shack's.
4. Press **Leave** in the banner when you are done. Nexus starts again as this PC's own station.

If another device holds the station, the banner says so and offers **Take control**. Taking it
ends the other device's operating: anything it is transmitting stops at once, and this PC starts
with TX off.

Over the internet, after 15 minutes with no click, key or PTT, the banner asks **Still there?**.
Press **I'm here**; with no answer the internet link ends a minute later, and **Reconnect** brings
it back.

### Check the station's key

The station's key is how you know the station on the other end is yours, and not something
pretending to be it.

1. At the shack, read **This station's key** under Remote access.
2. In the browser, read the key on the station's card; on the second PC, read **This station's
   key for the internet road** on its row.
3. Compare them group by group.
4. If they match, there is nothing to do in a browser. On the second PC, press **It matches**:
   the internet road carries nothing until you do.
5. If they do not match, do not connect. On the second PC press **It doesn't match**: a
   station paired with a code keeps its pairing on your network and loses only its internet
   road; a station you were invited to is forgotten.

A browser that later sees a different key for the station says so, shows both, and connects
nothing until you press **Accept the new key**. Accept it only if the new key matches the
shack's. If the shack's key is lost (its password store was cleared, say), use **Revoke station
access** there and pair again, then compare the new key everywhere.

### Stop remote use

- **One browser or PC:** at the shack, **Revoke browser approval** or **Remove** on its row.
  It is disconnected at once, and anything it kept on the air stops.
- **Everyone for now:** **Turn off Remote**. Approved devices get back what they had when you turn
  it on again, with the TX switch off.
- **Everyone, until you say otherwise:** **End remote control**.

### Download your data or delete your account

Sign in on the Nexus Remote page. The **Your account** card has two buttons:

- **Download my data** saves a copy of what Nexus Remote holds about your account as a JSON
  file: your trial and plan, your stations, their devices and your invitations.
- **Delete account** asks you to type DELETE, then removes the account, your stations with their
  devices and the invitations made there, and your devices at other people's stations. Every one
  of them is disconnected at once, and people you invited lose access. A paid plan is cancelled
  first, at once and without a refund for the rest of the period; if it cannot be cancelled,
  nothing is deleted. Nexus at your shack keeps working.

What is kept after a delete is what stops a second free trial (fingerprints of your address,
callsign and shack computer, and your trial dates) and the billing records the law requires.

## What needs the internet, and what is free

| | On the shack's own network | Over the internet |
|---|---|---|
| Second PC paired with a code | Works with no internet and no account | — |
| Second PC added by signing in | Works; signs in again every 30 days, which needs the Nexus Remote service | Through the Nexus Remote service |
| Browser stream | Stream and Listen from an approved browser on the same network stay free | Through the Nexus Remote service |
| Cost | Free | Free during the Beta; paid plans are coming |

Nexus Remote is free during the Beta. Paid plans for use over the internet are **coming**: they
are not on sale during the Beta, and this page will say how to subscribe once they are. On the
shack's own network Remote stays free. When an account has no internet use left, the second PC
and an approved browser on the same network still operate the station.

## Troubleshooting

| What you see | What it means | What to do |
|---|---|---|
| **Streaming is off at the shack** | **Stream this station from my browser** is off | Turn it on at the shack, then start the stream again |
| **This station's key has changed** | The service lists a different key than this browser kept | Compare with the shack; accept only if they match |
| **This reply was not signed with your station's key** | Something other than your station answered | Nothing connected; check the station's key at the shack |
| **This computer waits for approval at the station, under Remote access** | The PC is not approved for the internet road yet | Approve it at the shack, checking its key |
| **The internet road waits until you check the station's key** | You have not pressed **It matches** | [Check the station's key](#check-the-stations-key) |
| **The internet road is part of Nexus Remote's paid service, and your account does not include it now** | This account's internet use has ended | Operate on the shack's own network, which stays free |
| **This network lets no audio or second Stop road through** | A hotel or office network blocks the datagrams Stop's own road and audio need | Watch, tune and log; transmitting from here is off |
| **This computer runs Nexus … and the station runs Nexus …** | The two PCs run different versions | Update the one the banner names |
| **The station has eight devices already** | Eight browsers and computers are approved | Remove one at the shack |
| **Nexus did not find the station on this network** | Name lookup did not reach it, or the PC is elsewhere | Type the address the shack's card shows, or press **Not on this network** |
| The second PC cannot reach the station on your network | A network Windows calls Public, the firewall question cancelled, or guest Wi-Fi that keeps devices apart | Read what the shack's card says under **Remote over this network**, and use a home network set to Private |
| **Another program is using port 42078** | Something else holds the port Nexus signs in through | Close it, then sign in again |

For a stuck transmission, see [Scenarios & troubleshooting](scenarios.md).

## Honest limits

- **Beta.** Nexus Remote is a Beta, and access could be revoked at any time while it is.
- **Windows shacks only for the stream and Remote over this network;** the second PC is Windows
  only. Use a browser from Linux or macOS.
- **No paddle from afar.** CW from a second PC is the keyboard and macros.
- **Both PCs must run the same Nexus version.** The banner says which one to update.
- **Passwords and remote-access settings stay at the shack.** Approving, inviting, pairing and
  network settings cannot be changed through a stream or from a second PC.
- **A sign-in gives control, not transmit.** A browser or PC that signs in over the internet can
  listen and take control, and transmits only once you approve it at the shack, with its key. A
  computer paired on the shack's own network is never held. Stop always works, from every device.
- **No email notices yet.** Nexus does not yet email you when a browser or PC signs in to your
  station.
- **The station's licence class is the one checked.** Nexus at the shack applies the licence
  class set there, not a guest's. Someone you invite to operate transmits under your callsign.
- **A PC added by signing in needs the Nexus Remote service every 30 days** to stay on the
  station's network. If the service cannot be reached, pair that PC again with a code.
- **Over the internet, audio waits 120 ms** before playing (60 ms on your network) to ride out
  jitter, so the picture and audio are a little behind the shack.

## Related guides

- [Settings ▸ Remote access](settings-reference.md#remote-access) — every switch on the card.
- [Talk from Remote](phone.md#talk-from-remote) — your microphone on the rig, from the browser.
- [Scenarios & troubleshooting](scenarios.md) — stopping a transmission, and other jobs that cross chapters.
