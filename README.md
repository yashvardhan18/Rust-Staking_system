Simple staking for any SPL mint. Stake tokens, earn time-based rewards, and unstake after a lock period. Comes with program tests and a small TS client.

 ## Architecture

 ASCII diagram:
 ```
 User Wallet ──(stake tokens)──▶ Vault ATA (owner = Pool PDA)
        ▲                              │
        │                              │ rewards (claim)
        └──────────(unstake)───────────┘

                    ┌───────────────┐
                    │  StakingPool  │ (PDA: seeds ["pool", mint])
                    │ authority      │
                    │ vault (ATA)    │
                    │ reward_rate    │  (scaled 1e9)
                    │ min_lock_secs  │
                    │ total_staked   │
                    └───────────────┘

                    ┌───────────────┐
                    │  UserStake    │ (PDA: seeds ["user", pool, owner])
                    │ owner         │
                    │ pool          │
                    │ amount        │
                    │ start_time    │
                    │ last_claim    │
                    │ claimed_total │
                    └───────────────┘
 ```

### 🚀 Quickstart

 1. `cargo build-sbf --manifest-path program/Cargo.toml --sbf-out-dir dist`  
2. `solana program deploy dist/staking_program.so`  
3. `ts-node client/stake_client.ts`

## Clone & Setup on a new machine

### Prerequisites
- Rust toolchain (stable)
- Solana CLI v1.18+ configured to Devnet
  - `solana --version`
  - `solana config set --url https://api.devnet.solana.com`
  - Fund your default keypair: `solana airdrop 2` (or use `https://faucet.solana.com` if rate-limited)
- Node.js 18+ and npm
- ts-node (via `npx` or global)

### Steps
1) Clone the repo
```bash
git clone <YOUR_REPO_URL> solana-staking
cd solana-staking
```

2) Build the program (SBF)
```bash
cargo build-sbf --manifest-path program/Cargo.toml --sbf-out-dir dist
```

3) Deploy to Devnet and capture Program ID
```bash
solana program deploy dist/staking_program.so --output json | tee client/deploy.json
export PROGRAM_ID=$(cat client/deploy.json | sed -n 's/.*"programId" *: *"\([^"]*\)".*/\1/p')
echo $PROGRAM_ID
```

4) Install client deps and run the end-to-end script
```bash
cd client
npm i
PROGRAM_ID=$PROGRAM_ID npx ts-node stake_client.ts
npx ts-node update_readme.ts
cat devnet_output.json
```

5) (Optional) Run tests locally
```bash
cd ..
cargo test
```

Notes:
- The client uses your Solana CLI default keypair (`~/.config/solana/id.json`) as payer and user.
- If faucet airdrops are rate-limited, use an alternate Devnet faucet or transfer test SOL from another funded account.

 - One pool per SPL mint; rewards are paid from the same SPL mint.
 - Vault is the ATA of the Pool PDA for the mint.
 - Rewards formula: `pending = (elapsed * amount * reward_rate) / 1_000_000_000` using u128 math.

## Account Structures (short)

- StakingPool (112B): authority, vault, reward_rate (u64, 1e9 scale), min_lock_period (i64, s), total_staked, bump, reserved
- UserStake (104B): owner, pool, amount, start_time, last_claim_time, rewards_claimed, reserved

## Instructions (short)

- InitializePool(reward_rate, min_lock_period): create pool PDA + vault ATA; set config
- UpdateConfig({reward_rate?, min_lock_period?}): authority only; optional updates; logs
- InitializeUser: create user stake PDA for (pool, user)
- Stake(amount): transfer user ATA → vault; set times; update total; reject double-stake/zero
- ClaimRewards: pay pending since last_claim_time (u128 math); update times and claimed
- Unstake: require lock satisfied; auto-claim, then return principal; update total

 ## Security Considerations

 - PDAs derived with `Pubkey::find_program_address`.
 - Authority-only config updates.
 - All program-created accounts are checked for rent exemption; failure returns `NotRentExempt`.
 - Signer and ownership checks on all instructions.
 - Double-stake attempts rejected.
 - Overflow-safe arithmetic for rewards (u128 with checks).

 ## Build & Deploy (local SBF, no Docker)

 ```bash
 cargo build-sbf --manifest-path program/Cargo.toml --sbf-out-dir dist
 solana program deploy dist/staking_program.so
 solana airdrop 2
 ```

 ## CLI & Client Usage

 - Client script: `client/stake_client.ts`
   - Prints Program ID, PDAs (pool, user, vault)
   - Executes initialize, stake, claim, unstake on Devnet
   - Saves signatures to `client/devnet_output.json`

 Example (ts-node):
 ```bash
 export PROGRAM_ID=<YourProgramId>
 ts-node client/stake_client.ts
 ```

 ## Tests

 - Uses `solana-program-test`.
 - Scenarios covered:
   - Initialize Pool
   - Update Config (authorized) and unauthorized attempt (fail)
   - Initialize User
   - Stake SPL tokens
   - Early Unstake rejection
   - Claim rewards accuracy (tolerance)
   - Unstake after lock period
   - Multi-user concurrent stakes & claims

 Run:
 ```bash
 cargo test
 ```

## Program ID & Devnet Signatures

Current Devnet details:

 - Program ID: `BC4G4EqWVGdBhy1nPLWBF9fdkMdgyygMDjAV9ATcTWVx`
 - InitializePool: `2KHqGcT8X5bZPNHUp5KpU4YMp3ekNYeJnZtUcdxqTPKVgdSci1iQiTXQJAvpAEKpRHVKkkf1rfQw7kERAocVz8Bz`
 - Stake: `2DfjnBfEDEVM6F1APov3VFgcaguyV1XsAaj1kWtePC7gqYFG2abamG3hiTAXQtdB2XMgmnR2g33UGhEjxtpxswjG`
 - Claim: `3cxnqBb7kdiJ8ur7gWtuwnwBpjNXSmn8aWiA84F9pZ1Vutdej9ncXNF6F1LtfvzhYitgJRVvyfhmviSvPNi9F8sn`
 - Unstake: `3mXHH2KHKX6TQNvwXHyiKes1rK9uBok5tqSViftb3BZU6s864SfiYKRd39K1EakqQhgCfc9957Sbgna7ERDquzT1`

 ## Known Limitations & Notes

 - Rewards are paid from the same mint; ensure vault is pre-funded on Devnet.
 - Reward accuracy depends on cluster time; very short intervals may be small.
 - Single active stake per user per pool (simple model).




