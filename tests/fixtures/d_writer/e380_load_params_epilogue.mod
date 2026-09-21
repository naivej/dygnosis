// inventory: e380_load_params_epilogue
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
A = 1.0;
end;
load_params_and_steady_state('e380_params.txt');
