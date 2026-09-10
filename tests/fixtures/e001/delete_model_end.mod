// inventory: e001_delete_model_end, e001_cascade_missing_model_end, auto_fix_cascade_delete_model_end
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
