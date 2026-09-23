// inventory: e001_opener_var_reserved_ident
// `var y end;` is a declaration 7.1 accepts at check. `end` is not a reserved
// identifier. A use of the name inside a block is a different refuse.
var y end;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e;
end;

initval;
y = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
