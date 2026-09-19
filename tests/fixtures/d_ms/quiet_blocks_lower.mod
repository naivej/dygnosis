// inventory: d_ms_quiet_blocks_lower
// A `svar_identification` block closed by `lower_cholesky`, and a second
// `conditional_forecast_paths` block.
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
lower_cholesky;
end;
conditional_forecast_paths;
var k;
periods 1 2;
values 0.1 0.2;
end;
conditional_forecast_paths;
var y;
periods 3:4;
values 0.3;
end;
