// inventory: e001_delete_occbin_end
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
occbin_constraints;
name 'ELB';
bind y <= 0;
