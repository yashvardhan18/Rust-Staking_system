 import fs from 'fs';
 import path from 'path';

 const OUT_JSON = path.resolve(__dirname, 'devnet_output.json');
 const README = path.resolve(__dirname, '..', 'README.md');

 function main() {
   if (!fs.existsSync(OUT_JSON)) {
     console.error('No devnet_output.json found at', OUT_JSON);
     process.exit(1);
   }
   const data = JSON.parse(fs.readFileSync(OUT_JSON, 'utf8'));
   const programId = data.programId || '';
   const stake = data.txs?.stake || '';
   const claim = data.txs?.claim || '';
   const unstake = data.txs?.unstake || '';

   let readme = fs.readFileSync(README, 'utf8');
   readme = readme.replace('Program ID: `...`', `Program ID: \`${programId}\``);
   readme = readme.replace('InitializePool: `...`', `InitializePool: \`${data.txs?.initializePool || ''}\``);
   readme = readme.replace('Stake: `...`', `Stake: \`${stake}\``);
   readme = readme.replace('Claim: `...`', `Claim: \`${claim}\``);
   readme = readme.replace('Unstake: `...`', `Unstake: \`${unstake}\``);

   // Deliverables table replacements
   readme = readme.replace('`<PASTE PROGRAM ID HERE>`', `\`${programId}\``);
   readme = readme.replace('`<sig1>`', `\`${stake}\``);
   readme = readme.replace('`<sig2>`', `\`${claim}\``);
   readme = readme.replace('`<sig3>`', `\`${unstake}\``);

   fs.writeFileSync(README, readme);
   console.log('README.md updated with Devnet data.');
 }

 main();


