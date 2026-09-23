// inventory: e001_opener_var_skipped_block
// A name that reaches the parser only through `at_skipped_block`: declared and used,
// which 7.1 accepts.
var y priors;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + priors;
priors = 0.1 * y;
end;

initval;
y = 0;
priors = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
