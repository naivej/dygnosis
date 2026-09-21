// inventory: e380_load_params_after_unknown
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
load_params_and_steady_state('e380_after_params.txt');
epilogue;
A = 1.0;
end;
external_function(name='ef', nargs=1);
