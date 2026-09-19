// inventory: d_ms_quiet_blocks
// The two `… end;` blocks of the family, with every legal body row.
// 7.1 allows one `svar_identification` block per file and one of the two
// choleskys inside it, so the second cholesky and the second block shape sit in
// `quiet_blocks_lower.mod`.
var y c k;
varexo e;
parameters alpha beta delta;
alpha = 0.36;
beta = 0.99;
delta = 0.025;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = delta*y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
svar_identification;
exclusion constants;
exclusion lag 0;
equation 1, y, c;
equation 2, c;
exclusion lag 1;
equation 1, y;
upper_cholesky;
restriction equation 1, coeff(y,0) = 0;
restriction equation 2, coeff(c,1) = coeff(y,1);
end;
conditional_forecast_paths;
var y;
periods 1 2 3 4;
values 0.1 0.2 0.3 0.4;
var c;
periods 1:4;
values 0.5;
end;
