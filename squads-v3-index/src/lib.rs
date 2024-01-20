use solana_program::{
    account_info::AccountInfo,
    declare_id,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction, system_program,
};

declare_id!("idxqM2xnXsym7KL9YQmC8GG6TvdV9XxvHeMWdiswpwr");

pub mod squads_mpl {
    use solana_program::declare_id;
    declare_id!("SMPLecH534NA9acpos4G6x7uf3LWbCAwZQE9e8ZekMu");
}

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

#[track_caller]
#[inline(always)]
pub fn assert_with_msg(v: bool, err: impl Into<ProgramError>, msg: &str) -> ProgramResult {
    if v {
        Ok(())
    } else {
        let caller = std::panic::Location::caller();
        msg!("{}. \n{}", msg, caller);
        Err(err.into())
    }
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let system_program = &accounts[0];
    let authority = &accounts[1];
    let multisig = &accounts[2];
    let payer = &accounts[3];
    let index = &accounts[4];
    assert_with_msg(
        *system_program.key == system_program::id(),
        ProgramError::InvalidArgument,
        "Invalid system program",
    )?;
    assert_with_msg(
        payer.is_signer && payer.is_writable,
        ProgramError::InvalidArgument,
        "Payer must be a signer and writable",
    )?;

    let (index_key, bump) = Pubkey::find_program_address(&[authority.key.as_ref()], program_id);
    assert_with_msg(
        *index.key == index_key && index.is_writable,
        ProgramError::InvalidArgument,
        "Invalid index account",
    )?;

    assert_with_msg(
        *multisig.owner == squads_mpl::ID,
        ProgramError::IllegalOwner,
        "Multisig must be owned by Squads V3 program",
    )?;

    // Validate the multisig authority key.
    let (derived_authority_key, _) = Pubkey::find_program_address(
        &[
            b"squad",
            multisig.key.as_ref(),
            &1_u32.to_le_bytes(), // Authority index should just be 1
            b"authority",
        ],
        &squads_mpl::id(),
    );
    assert_with_msg(
        *authority.key == derived_authority_key,
        ProgramError::InvalidArgument,
        "Authority must be derived from the multisig",
    )?;

    // Validate the multisig account data.
    let bytes = multisig.data.borrow();
    let mut disc = [0_u8; 8];
    disc.copy_from_slice(&bytes[..8]);
    assert_with_msg(
        // This is the Anchor discriminant of the Squads V3 Multisig account.
        [70, 118, 9, 108, 254, 215, 31, 120] == disc,
        ProgramError::InvalidArgument,
        "Discriminator mismatch",
    )?;

    if index.data_is_empty() {
        let current_lamports = **index.try_borrow_lamports()?;
        if current_lamports == 0 {
            invoke_signed(
                &system_instruction::create_account(payer.key, index.key, 890880, 0, program_id),
                &accounts,
                &[&[authority.key.as_ref(), &[bump]]],
            )?;
        } else {
            // Fund the account for rent exemption.
            let required_lamports = 890880_u64.saturating_sub(current_lamports);
            if required_lamports > 0 {
                invoke(
                    &system_instruction::transfer(payer.key, index.key, required_lamports),
                    &accounts,
                )?;
            }
            // Allocate space.
            invoke_signed(
                &system_instruction::allocate(index.key, 0),
                &[index.clone(), system_program.clone()],
                &[&[authority.key.as_ref(), &[bump]]],
            )?;
            // Assign to the specified program
            invoke_signed(
                &system_instruction::assign(index.key, program_id),
                &[index.clone(), system_program.clone()],
                &[&[authority.key.as_ref(), &[bump]]],
            )?;
        }
    } else {
        msg!("Authority already indexed");
        return Ok(());
    }

    Ok(())
}
