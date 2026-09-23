// inventory: e001_opener_var_used
// A declared variable named after a block opener: 7.1 reads the declaration list
// through its identifier rule and accepts the file, so this must stay quiet.
var y shocks;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + shocks;
shocks = 0.1 * y;
end;

initval;
y = 0;
shocks = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
