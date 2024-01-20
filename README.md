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
