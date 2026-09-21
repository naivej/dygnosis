// inventory: quiet_load_params
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1) + e;
end;
load_params_and_steady_state('quiet_load_params.txt');
