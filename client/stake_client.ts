 import {
   Connection,
   Keypair,
   PublicKey,
   SystemProgram,
   Transaction,
   TransactionInstruction,
   sendAndConfirmTransaction,
   LAMPORTS_PER_SOL,
 } from '@solana/web3.js';
 import {
   getAssociatedTokenAddressSync,
   createAssociatedTokenAccountInstruction,
  createInitializeMintInstruction,
   createMintToInstruction,
   createTransferInstruction,
   getMint,
   TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
 } from '@solana/spl-token';
// We'll manually encode instruction data; no Anchor
import fs from 'fs';
import os from 'os';
import path from 'path';

 // Program ID: replace with the deployed program id
 const PROGRAM_ID = new PublicKey(process.env.PROGRAM_ID ?? 'BC4G4EqWVGdBhy1nPLWBF9fdkMdgyygMDjAV9ATcTWVx');

 // Simple discriminants (match Rust enum order)
 const IX = {
   InitializePool: 0,
   UpdateConfig: 1,
   InitializeUser: 2,
   Stake: 3,
   ClaimRewards: 4,
   Unstake: 5,
 } as const;

// Manual LE encoders for primitive types
const u8 = (n: number) => Buffer.from([n & 0xff]);
const u64le = (n: bigint) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(n); return b; };
const i64le = (n: bigint) => { const b = Buffer.alloc(8); b.writeBigInt64LE(n); return b; };

function encodeInitializePool(rr: bigint, lp: bigint): Buffer {
  return Buffer.concat([u8(IX.InitializePool), u64le(rr), i64le(lp)]);
}
function encodeUpdateConfig(rr: bigint | null, lp: bigint | null): Buffer {
  // Borsh Option<T>: 0x00 for None, 0x01 + T for Some
  const optU64 = rr === null ? u8(0) : Buffer.concat([u8(1), u64le(rr)]);
  const optI64 = lp === null ? u8(0) : Buffer.concat([u8(1), i64le(lp)]);
  return Buffer.concat([u8(IX.UpdateConfig), optU64, optI64]);
}
function encodeInitializeUser(): Buffer { return Buffer.from([IX.InitializeUser]); }
function encodeStake(amount: bigint): Buffer { return Buffer.concat([u8(IX.Stake), u64le(amount)]); }
function encodeNoArgs(tag: number): Buffer { return Buffer.from([tag]); }

 function findPoolPda(mint: PublicKey): [PublicKey, number] {
   return PublicKey.findProgramAddressSync([Buffer.from('pool'), mint.toBuffer()], PROGRAM_ID);
 }
 function findUserPda(pool: PublicKey, owner: PublicKey): [PublicKey, number] {
   return PublicKey.findProgramAddressSync([Buffer.from('user'), pool.toBuffer(), owner.toBuffer()], PROGRAM_ID);
 }

function loadKeypair(file?: string): Keypair {
  const keypairPath = file ?? path.join(os.homedir(), '.config', 'solana', 'id.json');
  const raw = fs.readFileSync(keypairPath, 'utf8');
  const secret = Uint8Array.from(JSON.parse(raw));
  return Keypair.fromSecretKey(secret);
}

 async function main() {
  const connection = new Connection(process.env.SOLANA_RPC ?? 'https://api.devnet.solana.com', 'confirmed');
  const payer = loadKeypair(process.env.KEYPAIR);

   // Create mint with payer as mint authority
   const mint = Keypair.generate();
  const MINT_SPACE = 82;
  const rent = await connection.getMinimumBalanceForRentExemption(MINT_SPACE);
  let tx = new Transaction().add(
    SystemProgram.createAccount({ fromPubkey: payer.publicKey, newAccountPubkey: mint.publicKey, lamports: rent, space: MINT_SPACE, programId: TOKEN_PROGRAM_ID }),
    createInitializeMintInstruction(mint.publicKey, 9, payer.publicKey, null)
  );
  let sigInitMint = await sendAndConfirmTransaction(connection, tx, [payer, mint]);

   // User accounts and ATAs
  const user = payer; // reuse payer as user to avoid faucet rate limits
   const userAta = getAssociatedTokenAddressSync(mint.publicKey, user.publicKey);
   const createAtaIx = createAssociatedTokenAccountInstruction(payer.publicKey, userAta, user.publicKey, mint.publicKey);
   await sendAndConfirmTransaction(connection, new Transaction().add(createAtaIx), [payer]);

   // Mint initial tokens to user
   const mintToIx = createMintToInstruction(mint.publicKey, userAta, payer.publicKey, 1_000_000_000_000n); // 1000 tokens (9dp)
   await sendAndConfirmTransaction(connection, new Transaction().add(mintToIx), [payer]);

   // Derive PDAs and ATAs
  const [poolPda] = findPoolPda(mint.publicKey);
  const [userStakePda] = findUserPda(poolPda, user.publicKey);
  const vaultAta = getAssociatedTokenAddressSync(mint.publicKey, poolPda, true);

   console.log('Program ID:', PROGRAM_ID.toBase58());
   console.log('Mint:', mint.publicKey.toBase58());
   console.log('Pool PDA:', poolPda.toBase58());
   console.log('User Stake PDA:', userStakePda.toBase58());
   console.log('Vault ATA:', vaultAta.toBase58());

   // InitializePool
  const initData = encodeInitializePool(5_000_000n, 10n);
   let initIx = new TransactionInstruction({
     programId: PROGRAM_ID,
     keys: [
       { pubkey: payer.publicKey, isSigner: true, isWritable: true },
       { pubkey: user.publicKey, isSigner: true, isWritable: false }, // authority
       { pubkey: poolPda, isSigner: false, isWritable: true },
       { pubkey: mint.publicKey, isSigner: false, isWritable: false },
       { pubkey: vaultAta, isSigner: false, isWritable: true },
       { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: ASSOCIATED_TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
       { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: new PublicKey('SysvarRent111111111111111111111111111111111'), isSigner: false, isWritable: false },
     ],
     data: initData,
   });
  let sigInitPool = await sendAndConfirmTransaction(connection, new Transaction().add(initIx), [payer, user]);
  console.log('InitializePool tx:', sigInitPool);

   // InitializeUser
   const initUserIx = new TransactionInstruction({
     programId: PROGRAM_ID,
     keys: [
       { pubkey: payer.publicKey, isSigner: true, isWritable: true },
       { pubkey: user.publicKey, isSigner: true, isWritable: false },
       { pubkey: poolPda, isSigner: false, isWritable: false },
       { pubkey: userStakePda, isSigner: false, isWritable: true },
       { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
       { pubkey: new PublicKey('SysvarRent111111111111111111111111111111111'), isSigner: false, isWritable: false },
     ],
     data: encodeInitializeUser(),
   });
  let sigInitUser = await sendAndConfirmTransaction(connection, new Transaction().add(initUserIx), [payer, user]);
  console.log('InitializeUser tx:', sigInitUser);

   // Stake
   const stakeIx = new TransactionInstruction({
     programId: PROGRAM_ID,
     keys: [
       { pubkey: user.publicKey, isSigner: true, isWritable: false },
       { pubkey: userAta, isSigner: false, isWritable: true },
       { pubkey: mint.publicKey, isSigner: false, isWritable: false },
      { pubkey: poolPda, isSigner: false, isWritable: true },
       { pubkey: userStakePda, isSigner: false, isWritable: true },
       { pubkey: vaultAta, isSigner: false, isWritable: true },
       { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
     ],
     data: encodeStake(100_000_000_000n),
   });
  let sigStake = await sendAndConfirmTransaction(connection, new Transaction().add(stakeIx), [payer, user]);
  console.log('Stake tx:', sigStake);

   // Fund vault for rewards (mint some extra to vault)
   const vaultCreateIx = createAssociatedTokenAccountInstruction(payer.publicKey, vaultAta, poolPda, mint.publicKey);
   try { await sendAndConfirmTransaction(connection, new Transaction().add(vaultCreateIx), [payer]); } catch {}
   const mintRewardIx = createMintToInstruction(mint.publicKey, vaultAta, payer.publicKey, 1_000_000_000_000n);
   await sendAndConfirmTransaction(connection, new Transaction().add(mintRewardIx), [payer]);

   // Claim
   const claimIx = new TransactionInstruction({
     programId: PROGRAM_ID,
     keys: [
       { pubkey: user.publicKey, isSigner: true, isWritable: false },
       { pubkey: userAta, isSigner: false, isWritable: true },
       { pubkey: mint.publicKey, isSigner: false, isWritable: false },
       { pubkey: userStakePda, isSigner: false, isWritable: true },
       { pubkey: poolPda, isSigner: false, isWritable: true },
       { pubkey: vaultAta, isSigner: false, isWritable: true },
       { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
     ],
     data: encodeNoArgs(4),
   });
  let sigClaim = await sendAndConfirmTransaction(connection, new Transaction().add(claimIx), [payer, user]);
  console.log('Claim tx:', sigClaim);

   // Unstake
  const sleep = (ms: number) => new Promise((res) => setTimeout(res, ms));
  // Wait for lock period (>10s) to elapse
  await sleep(12000);
   const unstakeIx = new TransactionInstruction({
     programId: PROGRAM_ID,
     keys: [
       { pubkey: user.publicKey, isSigner: true, isWritable: false },
       { pubkey: userAta, isSigner: false, isWritable: true },
       { pubkey: mint.publicKey, isSigner: false, isWritable: false },
       { pubkey: userStakePda, isSigner: false, isWritable: true },
       { pubkey: poolPda, isSigner: false, isWritable: true },
       { pubkey: vaultAta, isSigner: false, isWritable: true },
       { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
     ],
     data: encodeNoArgs(5),
   });
  let sigUnstake = await sendAndConfirmTransaction(connection, new Transaction().add(unstakeIx), [payer, user]);
  console.log('Unstake tx:', sigUnstake);

   // Save for README inclusion
  const out = {
    programId: PROGRAM_ID.toBase58(),
    mint: mint.publicKey.toBase58(),
    poolPda: poolPda.toBase58(),
    userStakePda: userStakePda.toBase58(),
    vaultAta: vaultAta.toBase58(),
    txs: {
      initializeMint: sigInitMint,
      initializePool: sigInitPool,
      initializeUser: sigInitUser,
      stake: sigStake,
      claim: sigClaim,
      unstake: sigUnstake,
    },
  };
  fs.writeFileSync(path.resolve(__dirname, 'devnet_output.json'), JSON.stringify(out, null, 2));
 }

 main().catch((e) => {
   console.error(e);
   process.exit(1);
 });


