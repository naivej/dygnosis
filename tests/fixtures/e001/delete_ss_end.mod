// inventory: e001_delete_ss_end
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
steady_state_model;
y = 0;
