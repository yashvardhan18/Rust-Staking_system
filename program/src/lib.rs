

 use borsh::{BorshDeserialize, BorshSerialize};
 use solana_program::{
     account_info::{next_account_info, AccountInfo},
     clock::Clock,
     entrypoint,
     entrypoint::ProgramResult,
     msg,
     program::{invoke, invoke_signed},
     program_error::ProgramError,
     program_pack::Pack,
     pubkey::Pubkey,
     rent::Rent,
     sysvar::Sysvar,
 };
 use spl_associated_token_account::instruction as ata_ix;
 use spl_token::instruction as token_ix;


 // Account size constants 
 // Keep these in sync with the structs below
 pub const STAKING_POOL_SIZE: usize = 112;
 pub const USER_STAKE_SIZE: usize = 104;

 pub const SEED_POOL: &[u8] = b"pool";
 pub const SEED_USER: &[u8] = b"user";



 #[derive(thiserror::Error, Debug, Copy, Clone)]
 pub enum StakingError {
     #[error("Unauthorized")] Unauthorized,
     #[error("NotRentExempt")] NotRentExempt,
     #[error("InvalidOwner")] InvalidOwner,
     #[error("InvalidMint")] InvalidMint,
     #[error("DoubleStake")] DoubleStake,
     #[error("ZeroAmount")] ZeroAmount,
     #[error("LockActive")] LockActive,
     #[error("Overflow")] Overflow,
     #[error("VaultInsufficient")] VaultInsufficient,
     #[error("ATAMissing")] ATAMissing,
     #[error("TimeWentBackwards")] TimeWentBackwards,
 }

 impl From<StakingError> for ProgramError {
     fn from(e: StakingError) -> Self {
         ProgramError::Custom(e as u32)
     }
 }


 /// StakingPool: One per mint. Holds authority, config and totals.
 #[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
 pub struct StakingPool {
     /// Admin authority that can update config
     pub authority: Pubkey, // 32
     /// Vault ATA (owner = pool PDA) for the staking mint
     pub vault: Pubkey,     // 32
     /// Reward rate per second per token staked (scaled by 1e9)
     pub reward_rate: u64,  // 8
     /// Minimum lock period in seconds
     pub min_lock_period: i64, // 8
     /// Total staked across all users
     pub total_staked: u64, // 8
     /// Bump for pool PDA
     pub bump: u8,          // 1
     /// Reserved padding to reach STAKING_POOL_SIZE
     pub _reserved: [u8; 23], // 23 => 32+32+8+8+8+1+23 = 112
 }

 impl StakingPool {
     pub fn new(
         authority: Pubkey,
         vault: Pubkey,
         reward_rate: u64,
         min_lock_period: i64,
         bump: u8,
     ) -> Self {
         Self {
             authority,
             vault,
             reward_rate,
             min_lock_period,
             total_staked: 0,
             bump,
             _reserved: [0u8; 23],
         }
     }
 }

 /// UserStake: Tracks a user's single active stake in a given pool
 #[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
 pub struct UserStake {
     /// User wallet owner
     pub owner: Pubkey, // 32
     /// Pool this user is staked in
     pub pool: Pubkey,  // 32
     /// Current staked amount (0 means not staked)
     pub amount: u64,   // 8
     /// Stake start unix timestamp
     pub start_time: i64, // 8
     /// Last timestamp rewards were claimed
     pub last_claim_time: i64, // 8
     /// Cumulative rewards claimed (informational)
     pub rewards_claimed: u64, // 8
     /// Reserved padding to reach USER_STAKE_SIZE
     pub _reserved: [u8; 8], // 8 => 32+32+8+8+8+8+8 = 104
 }

 impl Default for UserStake {
     fn default() -> Self {
         Self {
             owner: Pubkey::default(),
             pool: Pubkey::default(),
             amount: 0,
             start_time: 0,
             last_claim_time: 0,
             rewards_claimed: 0,
             _reserved: [0u8; 8],
         }
     }
 }

 #[derive(BorshSerialize, BorshDeserialize, Debug)]
 pub enum StakingInstruction {
     /// Initialize a pool for a given mint
     /// Accounts:
     /// - [signer, writable] payer
     /// - [signer] authority
     /// - [writable] pool_pda
     /// - [] mint
     /// - [writable] vault_ata (ATA owned by pool_pda)
     /// - [] token_program
     /// - [] associated_token_program
     /// - [] system_program
     /// - [] rent
     InitializePool { reward_rate: u64, min_lock_period: i64 },

     /// Update config fields (only authority)
     /// Accounts:
     /// - [signer] authority
     /// - [writable] pool_pda
     UpdateConfig { new_reward_rate: Option<u64>, new_min_lock_period: Option<i64> },

     /// Initialize user stake account
     /// Accounts:
     /// - [signer, writable] payer
     /// - [signer] user
     /// - [] pool_pda
     /// - [writable] user_stake_pda
     /// - [] system_program
     /// - [] rent
     InitializeUser,

     /// Stake a specific amount from user's ATA to pool vault
     /// Accounts:
     /// - [signer] user
     /// - [writable] user_ata
     /// - [] mint
     /// - [] pool_pda
     /// - [writable] user_stake_pda
     /// - [writable] vault_ata
     /// - [] token_program
     Stake { amount: u64 },

     /// Claim rewards from pool vault to user's ATA
     /// Accounts:
     /// - [signer] user
     /// - [writable] user_ata
     /// - [] mint
     /// - [writable] user_stake_pda
     /// - [writable] pool_pda
     /// - [writable] vault_ata
     /// - [] token_program
     ClaimRewards,

     /// Unstake principal back to user after lock period
     /// Accounts:
     /// - [signer] user
     /// - [writable] user_ata
     /// - [] mint
     /// - [writable] user_stake_pda
     /// - [writable] pool_pda
     /// - [writable] vault_ata
     /// - [] token_program
     Unstake,
 }

 entrypoint!(process_instruction);

 pub fn process_instruction(
     program_id: &Pubkey,
     accounts: &[AccountInfo],
     instruction_data: &[u8],
 ) -> ProgramResult {
     let ix = StakingInstruction::try_from_slice(instruction_data)
         .map_err(|_| ProgramError::InvalidInstructionData)?;
     match ix {
         StakingInstruction::InitializePool { reward_rate, min_lock_period } => {
             process_initialize_pool(program_id, accounts, reward_rate, min_lock_period)
         }
         StakingInstruction::UpdateConfig { new_reward_rate, new_min_lock_period } => {
             process_update_config(program_id, accounts, new_reward_rate, new_min_lock_period)
         }
         StakingInstruction::InitializeUser => process_initialize_user(program_id, accounts),
         StakingInstruction::Stake { amount } => process_stake(program_id, accounts, amount),
         StakingInstruction::ClaimRewards => process_claim(program_id, accounts),
         StakingInstruction::Unstake => process_unstake(program_id, accounts),
     }
 }

 fn find_pool_pda(program_id: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
     Pubkey::find_program_address(&[SEED_POOL, mint.as_ref()], program_id)
 }

 fn find_user_pda(program_id: &Pubkey, pool: &Pubkey, owner: &Pubkey) -> (Pubkey, u8) {
     Pubkey::find_program_address(&[SEED_USER, pool.as_ref(), owner.as_ref()], program_id)
 }

 // -------------------------------------------------------------------------------------
 // Instruction processors
 // -------------------------------------------------------------------------------------

 fn process_initialize_pool(
     program_id: &Pubkey,
     accounts: &[AccountInfo],
     reward_rate: u64,
     min_lock_period: i64,
 ) -> ProgramResult {
     let account_info_iter = &mut accounts.iter();
     let payer = next_account_info(account_info_iter)?; // signer, writable
     let authority = next_account_info(account_info_iter)?; // signer
     let pool_ai = next_account_info(account_info_iter)?; // writable
     let mint_ai = next_account_info(account_info_iter)?; // mint
     let vault_ai = next_account_info(account_info_iter)?; // writable ATA
     let token_program_ai = next_account_info(account_info_iter)?;
     let ata_program_ai = next_account_info(account_info_iter)?;
     let system_program_ai = next_account_info(account_info_iter)?;
     let rent_sysvar_ai = next_account_info(account_info_iter)?;

     // Signer checks
     if !payer.is_signer || !authority.is_signer {
         return Err(StakingError::Unauthorized.into());
     }

     // Derive expected pool PDA
     let (expected_pool, bump) = find_pool_pda(program_id, mint_ai.key);
     if *pool_ai.key != expected_pool {
         return Err(ProgramError::InvalidArgument);
     }

    // Create pool PDA account with program-derived signature if not already allocated
    if pool_ai.data_is_empty() {
         let rent = Rent::from_account_info(rent_sysvar_ai)?;
         let required_lamports = rent.minimum_balance(STAKING_POOL_SIZE);
         // Create the account
        let create_ix = solana_program::system_instruction::create_account(
             payer.key,
             pool_ai.key,
             required_lamports,
             STAKING_POOL_SIZE as u64,
             program_id,
         );
        let seeds: &[&[u8]] = &[SEED_POOL, mint_ai.key.as_ref(), &[bump]];
        invoke_signed(
            &create_ix,
            &[payer.clone(), pool_ai.clone(), system_program_ai.clone()],
            &[seeds],
        )?;

         // Sanity: rent exempt
         if !rent.is_exempt(pool_ai.lamports(), pool_ai.data_len()) {
             return Err(StakingError::NotRentExempt.into());
         }
     }

    // Create the vault ATA owned by pool PDA if not exists
     if vault_ai.data_is_empty() {
         let create_ata_ix = ata_ix::create_associated_token_account(
             payer.key,
             pool_ai.key,
             mint_ai.key,
             token_program_ai.key,
         );
        invoke(
             &create_ata_ix,
             &[
                 payer.clone(),
                 vault_ai.clone(),
                 pool_ai.clone(),
                 mint_ai.clone(),
                 system_program_ai.clone(),
                 token_program_ai.clone(),
                 ata_program_ai.clone(),
                 rent_sysvar_ai.clone(),
             ],
         )?;
     }

     // Persist pool state
     {
         // Verify vault ATA is indeed owned by pool PDA and for the given mint
         let vault_data = spl_token::state::Account::unpack(&vault_ai.try_borrow_data()?)
             .map_err(|_| ProgramError::InvalidAccountData)?;
         if vault_data.owner != *pool_ai.key {
             return Err(StakingError::InvalidOwner.into());
         }
         if vault_data.mint != *mint_ai.key {
             return Err(StakingError::InvalidMint.into());
         }

         let mut pool_data = StakingPool::new(*authority.key, *vault_ai.key, reward_rate, min_lock_period, bump);
         pool_data
             .serialize(&mut &mut pool_ai.data.borrow_mut()[..])
             .map_err(|_| ProgramError::AccountDataTooSmall)?;
     }

     msg!("Pool initialized. Authority={}, Rate={}, Lock={}s", authority.key, reward_rate, min_lock_period);
     Ok(())
 }

 fn process_update_config(
     program_id: &Pubkey,
     accounts: &[AccountInfo],
     new_reward_rate: Option<u64>,
     new_min_lock_period: Option<i64>,
 ) -> ProgramResult {
     let account_info_iter = &mut accounts.iter();
     let authority = next_account_info(account_info_iter)?; // signer
     let pool_ai = next_account_info(account_info_iter)?;   // writable

     if !authority.is_signer {
         return Err(StakingError::Unauthorized.into());
     }

     // Validate PDA data exists
     {
         let pool_data = pool_ai.try_borrow_data()?;
         if pool_data.is_empty() {
             return Err(ProgramError::UninitializedAccount);
         }
     }

     // We can't derive mint from pool directly; we trust PDA derivation by caller context.
     // Only enforce authority ownership here.
     let mut pool: StakingPool = StakingPool::try_from_slice(&pool_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;

     if pool.authority != *authority.key {
         return Err(StakingError::Unauthorized.into());
     }

     if let Some(rr) = new_reward_rate {
         pool.reward_rate = rr;
     }
     if let Some(lp) = new_min_lock_period {
         pool.min_lock_period = lp;
     }

     pool.serialize(&mut &mut pool_ai.data.borrow_mut()[..])
         .map_err(|_| ProgramError::AccountDataTooSmall)?;

     msg!(
         "Config updated: reward_rate={:?}, min_lock_period={:?}",
         new_reward_rate, new_min_lock_period
     );
     Ok(())
 }

 fn process_initialize_user(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
     let account_info_iter = &mut accounts.iter();
     let payer = next_account_info(account_info_iter)?; // signer, writable
     let user = next_account_info(account_info_iter)?;  // signer
     let pool_ai = next_account_info(account_info_iter)?; // read-only
     let user_stake_ai = next_account_info(account_info_iter)?; // writable
     let system_program_ai = next_account_info(account_info_iter)?;
     let rent_sysvar_ai = next_account_info(account_info_iter)?;

     if !payer.is_signer || !user.is_signer {
         return Err(StakingError::Unauthorized.into());
     }

     // Derive expected user stake PDA
     let (expected_user_pda, _bump) = find_user_pda(program_id, pool_ai.key, user.key);
     if *user_stake_ai.key != expected_user_pda {
         return Err(ProgramError::InvalidArgument);
     }

     // Create user stake PDA account using program-derived signature
     if user_stake_ai.data_is_empty() {
         let rent = Rent::from_account_info(rent_sysvar_ai)?;
         let required_lamports = rent.minimum_balance(USER_STAKE_SIZE);
         let create_ix = solana_program::system_instruction::create_account(
             payer.key,
             user_stake_ai.key,
             required_lamports,
             USER_STAKE_SIZE as u64,
             program_id,
         );
         let (_expected, user_bump) = find_user_pda(program_id, pool_ai.key, user.key);
         let seeds: &[&[u8]] = &[SEED_USER, pool_ai.key.as_ref(), user.key.as_ref(), &[user_bump]];
         invoke_signed(
             &create_ix,
             &[payer.clone(), user_stake_ai.clone(), system_program_ai.clone()],
             &[seeds],
         )?;
         if !rent.is_exempt(user_stake_ai.lamports(), user_stake_ai.data_len()) {
             return Err(StakingError::NotRentExempt.into());
         }
     }

     // Initialize zeroed user stake
     let mut us = UserStake::default();
     us.owner = *user.key;
     us.pool = *pool_ai.key;
     us.serialize(&mut &mut user_stake_ai.data.borrow_mut()[..])
         .map_err(|_| ProgramError::AccountDataTooSmall)?;

     msg!("User stake initialized for {}", user.key);
     Ok(())
 }

 fn process_stake(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64) -> ProgramResult {
     if amount == 0 {
         return Err(StakingError::ZeroAmount.into());
     }

     let account_info_iter = &mut accounts.iter();
     let user = next_account_info(account_info_iter)?; // signer
     let user_ata = next_account_info(account_info_iter)?; // writable
     let mint_ai = next_account_info(account_info_iter)?; // read-only
     let pool_ai = next_account_info(account_info_iter)?; // read-only
     let user_stake_ai = next_account_info(account_info_iter)?; // writable
     let vault_ai = next_account_info(account_info_iter)?; // writable
     let token_program_ai = next_account_info(account_info_iter)?;

     if !user.is_signer {
         return Err(StakingError::Unauthorized.into());
     }

     // Validate PDAs
     let (expected_user_pda, _) = find_user_pda(program_id, pool_ai.key, user.key);
     if *user_stake_ai.key != expected_user_pda {
         return Err(ProgramError::InvalidArgument);
     }

     let mut pool: StakingPool = StakingPool::try_from_slice(&pool_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;

     // Verify vault ATA matches pool config
     let vault_data = spl_token::state::Account::unpack(&vault_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     if vault_data.owner != *pool_ai.key {
         return Err(StakingError::InvalidOwner.into());
     }
     if vault_data.mint != *mint_ai.key || pool.vault != *vault_ai.key {
         return Err(StakingError::InvalidMint.into());
     }

     // Verify user's ATA is for the same mint and owned by the user
     let user_ata_data = spl_token::state::Account::unpack(&user_ata.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     if user_ata_data.owner != *user.key {
         return Err(StakingError::InvalidOwner.into());
     }
     if user_ata_data.mint != *mint_ai.key {
         return Err(StakingError::InvalidMint.into());
     }
     if user_ata_data.amount < amount {
         return Err(StakingError::VaultInsufficient.into()); // user insufficient balance
     }

     // Load user stake and ensure not already staked
     let mut us: UserStake = UserStake::try_from_slice(&user_stake_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     if us.amount != 0 {
         return Err(StakingError::DoubleStake.into());
     }
     if us.owner != *user.key || us.pool != *pool_ai.key {
         return Err(StakingError::InvalidOwner.into());
     }

     // Transfer user's tokens into the pool vault (authority = user)
     let transfer_ix = token_ix::transfer(
         token_program_ai.key,
         user_ata.key,
         vault_ai.key,
         user.key,
         &[],
         amount,
     )?;
     invoke(&transfer_ix, &[user_ata.clone(), vault_ai.clone(), user.clone(), token_program_ai.clone()])?;

     // Update user stake and pool totals
     let now = Clock::get()?.unix_timestamp;
     us.amount = amount;
     us.start_time = now;
     us.last_claim_time = now;
     us.serialize(&mut &mut user_stake_ai.data.borrow_mut()[..])
         .map_err(|_| ProgramError::AccountDataTooSmall)?;

     pool.total_staked = pool
         .total_staked
         .checked_add(amount)
         .ok_or(StakingError::Overflow)?;
     pool.serialize(&mut &mut pool_ai.data.borrow_mut()[..])
         .map_err(|_| ProgramError::AccountDataTooSmall)?;

     msg!("Staked: {} tokens by {}", amount, user.key);
     Ok(())
 }

 fn process_claim(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
     let account_info_iter = &mut accounts.iter();
     let user = next_account_info(account_info_iter)?; // signer
     let user_ata = next_account_info(account_info_iter)?; // writable
     let mint_ai = next_account_info(account_info_iter)?; // read-only
     let user_stake_ai = next_account_info(account_info_iter)?; // writable
     let pool_ai = next_account_info(account_info_iter)?; // writable
     let vault_ai = next_account_info(account_info_iter)?; // writable
     let token_program_ai = next_account_info(account_info_iter)?;

     if !user.is_signer {
         return Err(StakingError::Unauthorized.into());
     }

     let mut pool: StakingPool = StakingPool::try_from_slice(&pool_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     let mut us: UserStake = UserStake::try_from_slice(&user_stake_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;

     if us.owner != *user.key || us.pool != *pool_ai.key {
         return Err(StakingError::InvalidOwner.into());
     }

     // Verify token accounts and mint
     let vault_data = spl_token::state::Account::unpack(&vault_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     let user_ata_data = spl_token::state::Account::unpack(&user_ata.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     if vault_data.owner != *pool_ai.key || pool.vault != *vault_ai.key {
         return Err(StakingError::InvalidOwner.into());
     }
     if vault_data.mint != *mint_ai.key || user_ata_data.mint != *mint_ai.key {
         return Err(StakingError::InvalidMint.into());
     }
     if user_ata_data.owner != *user.key {
         return Err(StakingError::InvalidOwner.into());
     }

     let now = Clock::get()?.unix_timestamp;
     if now < us.last_claim_time {
         return Err(StakingError::TimeWentBackwards.into());
     }
     if us.amount == 0 {
         // Nothing to claim
         return Ok(());
     }

     let elapsed = (now - us.last_claim_time) as u128;
     let amount = us.amount as u128;
     let rate = pool.reward_rate as u128;
     let pending = elapsed
         .checked_mul(amount).ok_or(StakingError::Overflow)?
         .checked_mul(rate).ok_or(StakingError::Overflow)?
         / 1_000_000_000u128;
     let pending_u64: u64 = pending.try_into().map_err(|_| StakingError::Overflow)?;

     if pending_u64 > 0 {
         if vault_data.amount < pending_u64 {
             return Err(StakingError::VaultInsufficient.into());
         }

         // Transfer reward from vault to user ATA, signed by pool PDA
         let transfer_ix = token_ix::transfer(
             token_program_ai.key,
             vault_ai.key,
             user_ata.key,
             pool_ai.key,
             &[],
             pending_u64,
         )?;
         let (expected_pool, bump) = find_pool_pda(program_id, &vault_data.mint);
         if *pool_ai.key != expected_pool {
             return Err(ProgramError::InvalidArgument);
         }
         let seeds: &[&[u8]] = &[SEED_POOL, vault_data.mint.as_ref(), &[bump]];
         invoke_signed(
             &transfer_ix,
             &[vault_ai.clone(), user_ata.clone(), pool_ai.clone(), token_program_ai.clone()],
             &[seeds],
         )?;

         us.rewards_claimed = us
             .rewards_claimed
             .checked_add(pending_u64)
             .ok_or(StakingError::Overflow)?;
     }

     us.last_claim_time = now;
     us.serialize(&mut &mut user_stake_ai.data.borrow_mut()[..])
         .map_err(|_| ProgramError::AccountDataTooSmall)?;

     msg!("Rewards claimed: {} by {}", pending_u64, user.key);
     Ok(())
 }

 fn process_unstake(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
     let account_info_iter = &mut accounts.iter();
     let user = next_account_info(account_info_iter)?; // signer
     let user_ata = next_account_info(account_info_iter)?; // writable
     let mint_ai = next_account_info(account_info_iter)?; // read-only
     let user_stake_ai = next_account_info(account_info_iter)?; // writable
     let pool_ai = next_account_info(account_info_iter)?; // writable
     let vault_ai = next_account_info(account_info_iter)?; // writable
     let token_program_ai = next_account_info(account_info_iter)?;

     if !user.is_signer {
         return Err(StakingError::Unauthorized.into());
     }

     let mut pool: StakingPool = StakingPool::try_from_slice(&pool_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     let mut us: UserStake = UserStake::try_from_slice(&user_stake_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;

     if us.owner != *user.key || us.pool != *pool_ai.key {
         return Err(StakingError::InvalidOwner.into());
     }

     // Verify token accounts and mint
     let vault_data = spl_token::state::Account::unpack(&vault_ai.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     let user_ata_data = spl_token::state::Account::unpack(&user_ata.try_borrow_data()?)
         .map_err(|_| ProgramError::InvalidAccountData)?;
     if vault_data.owner != *pool_ai.key || pool.vault != *vault_ai.key {
         return Err(StakingError::InvalidOwner.into());
     }
     if vault_data.mint != *mint_ai.key || user_ata_data.mint != *mint_ai.key {
         return Err(StakingError::InvalidMint.into());
     }
     if user_ata_data.owner != *user.key {
         return Err(StakingError::InvalidOwner.into());
     }

     let now = Clock::get()?.unix_timestamp;
     if now < us.start_time {
         return Err(StakingError::TimeWentBackwards.into());
     }
     let staked = us.amount;
     if staked == 0 {
         return Ok(());
     }
     let elapsed = now - us.start_time;
     if elapsed < pool.min_lock_period {
         return Err(StakingError::LockActive.into());
     }

     // First, settle any pending rewards to keep accounting consistent
     // Reuse claim logic inline for simplicity
     {
         if now < us.last_claim_time {
             return Err(StakingError::TimeWentBackwards.into());
         }
         let elapsed_reward = (now - us.last_claim_time) as u128;
         let pending = elapsed_reward
             .checked_mul(us.amount as u128).ok_or(StakingError::Overflow)?
             .checked_mul(pool.reward_rate as u128).ok_or(StakingError::Overflow)?
             / 1_000_000_000u128;
         let pending_u64: u64 = pending.try_into().map_err(|_| StakingError::Overflow)?;
         if pending_u64 > 0 {
             if vault_data.amount < pending_u64 {
                 return Err(StakingError::VaultInsufficient.into());
             }
             let transfer_ix = token_ix::transfer(
                 token_program_ai.key,
                 vault_ai.key,
                 user_ata.key,
                 pool_ai.key,
                 &[],
                 pending_u64,
             )?;
             let (expected_pool, bump) = find_pool_pda(program_id, &vault_data.mint);
             if *pool_ai.key != expected_pool {
                 return Err(ProgramError::InvalidArgument);
             }
             let seeds: &[&[u8]] = &[SEED_POOL, vault_data.mint.as_ref(), &[bump]];
             invoke_signed(
                 &transfer_ix,
                 &[vault_ai.clone(), user_ata.clone(), pool_ai.clone(), token_program_ai.clone()],
                 &[seeds],
             )?;
             us.rewards_claimed = us
                 .rewards_claimed
                 .checked_add(pending_u64)
                 .ok_or(StakingError::Overflow)?;
         }
     }

     // Now return principal
     if vault_data.amount < staked {
         return Err(StakingError::VaultInsufficient.into());
     }
     let transfer_ix = token_ix::transfer(
         token_program_ai.key,
         vault_ai.key,
         user_ata.key,
         pool_ai.key,
         &[],
         staked,
     )?;
     let (expected_pool, bump) = find_pool_pda(program_id, &vault_data.mint);
     if *pool_ai.key != expected_pool {
         return Err(ProgramError::InvalidArgument);
     }
     let seeds: &[&[u8]] = &[SEED_POOL, vault_data.mint.as_ref(), &[bump]];
     invoke_signed(
         &transfer_ix,
         &[vault_ai.clone(), user_ata.clone(), pool_ai.clone(), token_program_ai.clone()],
         &[seeds],
     )?;

     // Update states
     us.amount = 0;
     us.start_time = 0;
     us.last_claim_time = 0;
     us.serialize(&mut &mut user_stake_ai.data.borrow_mut()[..])
         .map_err(|_| ProgramError::AccountDataTooSmall)?;

     pool.total_staked = pool
         .total_staked
         .checked_sub(staked)
         .ok_or(StakingError::Overflow)?;
     pool.serialize(&mut &mut pool_ai.data.borrow_mut()[..])
         .map_err(|_| ProgramError::AccountDataTooSmall)?;

     msg!("Unstaked: {} returned to {}", staked, user.key);
     Ok(())
 }


