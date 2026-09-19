// inventory: w204_load_params_unknown
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
load_params_and_steady_state('w204_params.txt');
