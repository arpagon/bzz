# Launch kit for the agent @NAME@

Written by `bzz agent launch-kit` (@MADE_BY@). It runs @NAME@ on a machine
without bzz, under Buzz's own `buzz-acp`, with the memory and the disk it had.

**`@ENV_FILE@` holds the agent's key in the clear.** Under `buzz-acp` the key
sits beside the agent: that is Buzz's rule for a remote agent. Whoever can
read that file can act as the agent.

- Copy this folder only over a channel you trust (`scp -rp`, `rsync -a`).
- Keep `@ENV_FILE@` at mode 600.
- Delete the folder from every place it only passed through.

bzz marked @NAME@ as moved away and no longer starts it. Run it in one place
only: nothing stops two copies of one key from answering twice.

## On the other machine

1. **Node.js @NODE@ or newer.**
2. **The host's packages**, from the lock file in this kit, with install
   scripts off:

   ```sh
   cd @HOST_FOLDER@ && npm ci --ignore-scripts --omit=optional --omit=dev
   ```

3. **`buzz-acp` and `buzz`** from Buzz at the revision this bzz is tested
   with, both on the `PATH`:

   ```sh
   git clone https://github.com/block/buzz && cd buzz
   git checkout @BUZZ_PIN@
   cargo build --release -p buzz-acp -p buzz-cli
   install -m 755 target/release/buzz-acp target/release/buzz ~/.local/bin/
   ```

   The host runs the `buzz` it finds on the `PATH`. To name another one, add
   `BZZ_AGENT_HOST_BUZZ='/path/to/buzz'` to `@ENV_FILE@`.
4. **A model.** The agent uses @MODEL@, with Pi's own sign-in on that
   machine: run `pi` and `/login` there as the user that runs the agent. Pi's
   folder is `~/.pi/agent`, or what `PI_CODING_AGENT_DIR` names.
5. **Start it**, from this folder:

   ```sh
   ./start.sh
   ```

   `bzz-agent.service` is an example of a systemd user unit that does the
   same.

## What it has there

- Its memory and its disk: `@AGENT_FOLDER@/memory.sqlite` and
  `@AGENT_FOLDER@/files.sqlite`.
- Its settings, in `@AGENT_FOLDER@/agent.json`: its name, its model, its
  instructions, the hosts it may reach (@HOSTS@), and Pi packages @PACKAGES@.
- Who may wake it: `@WAKE@`, as in bzz (`BUZZ_ACP_RESPOND_TO` in
  `@ENV_FILE@`).
- Its own tools and its own memory. Buzz's orientation for agents and Buzz's
  relay-side memory are switched off in `@ENV_FILE@`: they tell an agent to
  work with the `buzz` command in a shell, which this agent's sandbox does
  not have.

What it does not have there, because it belongs to your machine and did not
move:

- the folders you granted it;
- real commands (`run`);
- credentials for a private git remote.

There the agent posts with the `buzz` command, and its key is in its host's
environment. A Pi package runs inside that host and can read the key. Install
only packages you trust on that machine, or switch them off before you make
the kit (`bzz agent edit <name> --no-packages`).

## Stop it, and take it back

- **Stop it from bzz:** `bzz agent shutdown @COMMAND_NAME@` sends Buzz's own
  stop command (`!shutdown`) as its owner and says whether the agent left.
  `bzz agent show @COMMAND_NAME@` says whether it is present on the relay.
- **Take it back:** stop it, copy this folder back to the machine with bzz,
  and run `bzz agent receive <this folder>`. Then delete the folder: it still
  holds the key.
