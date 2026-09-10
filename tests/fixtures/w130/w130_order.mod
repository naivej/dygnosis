// inventory: w130_logn_before_n
var y n log_n;
varexo e;
parameters rho n_ss;
rho = 0.9;
n_ss = 1;
model;
y = rho * y(-1) + e;
end;
steady_state_model;
log_n = log(n);
n = n_ss;
y = 0;
end;
