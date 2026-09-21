// inventory: e380_load_params_used_trend
trend_var(growth_factor=1.02) A;
var(deflator=A) y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1) + e;
end;
load_params_and_steady_state('e380_used_trend_params.txt');
