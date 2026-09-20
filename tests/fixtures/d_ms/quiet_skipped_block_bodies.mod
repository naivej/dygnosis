// inventory: quiet_skipped_block_bodies
// Every pin `DYNARE_BLOCK` opener whose body this parser does not read. The rows are bare
// expressions or bare names, and a top-level recogniser must not look inside: `matched_moments;`
// rows are `ln_c;`-style expressions, and `priors;` rows are bare parameter names. 7.1 accepts
// each body, so no Error from the sweep may fire.
var y c k R Pie Y;
varexo e eps;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
R = beta*R(-1) + eps;
Pie = alpha*R(-1) + eps;
Y = beta*Pie(-1) + eps;
end;
initval;
y = 0;
c = 0;
k = 0;
R = 0;
Pie = 0;
Y = 0;
end;
shocks;
var e; stderr 0.1;
var eps; stderr 0.1;
end;
matched_moments;
y;
c;
c*y;
end;
priors;
alpha;
beta;
end;
