// inventory: equations_tags_dup_name
var y x z;
varexo e;
parameters a b;
a = 0.5;
b = 0.9;

model;
[name='policy'] y = a * y(-1) + e;
[name='policy'] x = b * x(+1);
z = y + x;
end;
