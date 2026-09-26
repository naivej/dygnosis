var y;
@#define is = 1:2
model;
@#for i in is
y = y(-1);
@#endfor
end;
