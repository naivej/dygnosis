// inventory: p_pac_quiet_deterministic
var y;
parameters a b;
a = 0.5;
b = 0.1;
model;
  y = a*y(-1);
end;
deterministic_trends;
  y (a+b);
end;
