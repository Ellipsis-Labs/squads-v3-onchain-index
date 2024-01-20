# squads-v3-onchain-index
A smart contract that allows users to directly associate program upgrade authority keys to the Squads V3 program.

## Problem
Today on Solana mainnet, it's essentially impossible to know how an on-chain program is being controlled and how the update authority is managed.

Program management generally falls into 3 categories:
1. Upgradeable by single hot wallet or ledger (👎)
2. Upgradeable by multisig via [Squads V3](https://github.com/Squads-Protocol/squads-mpl/tree/main/programs/squads-mpl) (👌)
3. Completely frozen (👍)

In practice, most protocol teams opt for the multisig approach because it's a reasonable tradeoff between security and control. However, it's extremely difficult to be able to know whether a program is in category 1 or 2.

## Solution
The `squads-v3-index` program allows anyone to permissionlessly create a link between program upgrade authorities and the Squads V3 program. After a program authority has been registered, it will be extremely easy for downstream applications to tie programs to Squads V3.

The program is deploy immutably to Solana mainnet at `idxqM2xnXsym7KL9YQmC8GG6TvdV9XxvHeMWdiswpwr`

To verify this program, run:
```bash
solana-verify verify-from-repo --remote -um --program-id idxqM2xnXsym7KL9YQmC8GG6TvdV9XxvHeMWdiswpwr --library-name squads_v3_index --mount-path squads-v3-index/ https://github.com/Ellipsis-Labs/squads-v3-onchain-index
```


## Installation
To install the CLI run:
```bash
cargo install squads-v3-index-cli
```

## Usage
If you want to index a program, you will first need to find the address of the Multisig Account associated with the program. **Note that this is different from the upgrade authority**.

Then pass call the `index` subcommand in the CLI. It will print the following for you to confirm:
```bash
$ squads-v3-index-cli 6x3BDkL2n7VjBWxRD95EsbQi2R2E4zxrvcz1VA6pihnK
3/4 Multisig account exists
Multisig key: 6x3BDkL2n7VjBWxRD95EsbQi2R2E4zxrvcz1VA6pihnK
Authority key: 8mv7G3fJq5a5ej7E14vgcSGeQKH79emjU9fVfuhyitEq

Executing instruction:

Instruction {
    program_id: idxqM2xnXsym7KL9YQmC8GG6TvdV9XxvHeMWdiswpwr,
    accounts: [
        AccountMeta {
            pubkey: 11111111111111111111111111111111,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: 8mv7G3fJq5a5ej7E14vgcSGeQKH79emjU9fVfuhyitEq,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: 6x3BDkL2n7VjBWxRD95EsbQi2R2E4zxrvcz1VA6pihnK,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: 2Hwmox2Qd84ZxPhKUGkTs7KUpjzYHWfHWbPT1kWvMf5b,
            is_signer: true,
            is_writable: false,
        },
        AccountMeta {
            pubkey: HwLnWCj5huUdzXnt2QmVUFrFcjZw7L7UJ1Paqz14q4zu,
            is_signer: false,
            is_writable: true,
        },
    ],
    data: [],
}

Cost: 0.00089588 SOL

(y/n)
```
If you don't want the confirmation you can pass in the `-y` flag to immediately execute.

After execution you can run the `check` subcommand on the program upgrade authority to validate that the index has been created:
```bash
$ squads-v3-index-cli check 8mv7G3fJq5a5ej7E14vgcSGeQKH79emjU9fVfuhyitEq
Index account exists for 8mv7G3fJq5a5ej7E14vgcSGeQKH79emjU9fVfuhyitEq ✅
```
