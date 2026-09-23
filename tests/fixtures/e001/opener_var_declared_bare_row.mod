// inventory: e001_opener_var_declared_bare_row
// The `declared_before` clause, on its own: a **bare** declared opener-named row in
// the middle of the model body. 7.1 reads it as the variable (`shocks;` is an
// equation with a zero right-hand side) and accepts the file, so the row must not be
// taken for a block. An assignment row would not show this: `shocks = 0.1*y;` is
// already protected by the `;` that follows the word.
var y shocks;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + shocks;
shocks;
y = y(-1);
end;

initval;
y = 0;
shocks = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
