// inventory: e063_for_var_after_endfor
@#for i in 1:2
@#endfor
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + @{i};
end;
