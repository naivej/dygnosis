// inventory: e378_assignment_not_parameter
// `y = 3;` with a declared `y` — 7.1's `init_param` runs `check_symbol_is_parameter`, so the file is refused with `y is not a parameter`.
var y c k;
varexo e;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
y = 3;
