// inventory: e001_opener_var_reserved_ident, e001_opener_var_reserved_ident_end
// Close call 2: a declared name whose spelling is in `DYNARE_COMMANDS` (16 words) or
// `RESERVED_BLOCK_KEYWORDS` (13). 7.1 accepts this declaration, and we report a false
// `Invalid Dynare identifier`. Slice 06b owns the fix.
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
