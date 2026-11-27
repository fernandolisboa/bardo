---
id: world
title: Mundo e personagens
group: production
place: world
tour: world
---

# Mundo e personagens

O **Mundo** guarda os personagens que voltam nos seus vídeos: uma avó que conta as histórias da família, um detetive, um mascote. Cada um tem uma aparência fixa, notas que sempre ou nunca valem e uma **imagem de referência** por visual, para que todo vídeo que escolhe o personagem mostre o mesmo rosto. Os personagens do projeto ficam em [Mundo](bardo:go/world); os que qualquer projeto pode escolher ficam na [aba Mundo da biblioteca global](bardo:go/library/world). Escrever e editar personagens não custa nada; desenhar uma imagem é pago e mostra o custo antes. [Mostre a tela para mim](bardo:tour/world).

<a id="characters"></a>
## Seus personagens

A lista guarda os personagens do projeto aberto (ou da biblioteca global, na aba dela). **Novo personagem** abre um formulário vazio; escolher um personagem o abre para edição. Os arquivados ficam escondidos até você marcar **Mostrar arquivados**. O nome de um personagem é único entre os personagens do mesmo lugar, arquivados incluídos, sem diferenciar maiúsculas: um projeto e a biblioteca global podem ter, cada um, uma Rosa.

<a id="appearance"></a>
## Aparência e notas

- **Nome** (obrigatório): no máximo 60 caracteres. O plano de cenas chama o personagem por esse nome.
- **Aparência** (obrigatória): rosto, cabelo, porte e roupas, no máximo 1.500 caracteres. Toda imagem de referência é desenhada a partir deste texto, e o plano de cenas o copia como está para o elenco.
- **Sempre** e **Nunca**: o que sempre vale (carrega um guarda-chuva vermelho) e o que nunca acontece (nunca fala em cena), no máximo 1.000 caracteres cada. O planejador de cenas as lê em todo vídeo que escolhe o personagem.

Uma nova **aparência** descarta as imagens de referência do personagem em todos os visuais e os retratos dos vídeos que o mostram, porque deixam de corresponder. Antes de salvar, o Bardo diz quais visuais e quantos vídeos: **Salvar e descartar** confirma. Cada imagem é desenhada de novo, e paga de novo, na próxima vez em que for preciso. Um novo nome ou novas notas mantêm as imagens.

<a id="pictures"></a>
## Imagens de referência

Uma imagem de referência é o retrato do personagem em um **visual**: um estilo do catálogo (foto, desenho animado, mangá e assim por diante), ou **as notas de um canal**, identificadas pelo canal e pelo texto das notas de estética dele. Cada imagem tem seu próprio registro de como foi feita (provedor, modelo, prompt, quando, e se um vídeo a desenhou, você a desenhou aqui ou a salvou do elenco de um vídeo), e fica na pasta `World` do projeto, ou em `Global\World` dentro da pasta base para a biblioteca global.

- **Antes da primeira cena que precisa dela**: um vídeo que mostra o personagem em um visual sem imagem desenha uma antes dessa cena, como faz com qualquer [pessoa que volta](scenes.md#cast), paga nesse vídeo. A imagem então fica guardada no mundo.
- **Sob demanda**: escolha um visual, veja a estimativa e clique em **Desenhar imagem**. A nova imagem aguarda revisão: **Manter** faz dela a que os vídeos nesse visual usam, no lugar da anterior, e **Descartar** a joga fora. Ela é paga no projeto do personagem; a de um personagem global, em nenhum projeto.

Todo vídeo seguinte no mesmo visual recebe a imagem mantida de graça: a estimativa não a conta e nenhuma chamada paga é feita. Outras notas em um canal são outro visual, então desenham uma nova imagem. Excluir o vídeo que desenhou a imagem primeiro mantém a imagem. **Remover** descarta uma imagem mantida; o próximo vídeo nesse visual desenha uma nova.

<a id="archive"></a>
## Duplicar, arquivar e excluir

- **Duplicar** copia os campos e as imagens mantidas com um novo nome.
- **Arquivar** esconde o personagem de [Quem aparece](scenes.md#who-appears); os vídeos que já o escolheram continuam com ele. **Restaurar** o traz de volta.
- **Excluir** remove o personagem com suas imagens. É recusado enquanto um vídeo em andamento o usa, e a mensagem cita esses vídeos: arquive o personagem, ou arquive ou exclua esses vídeos antes. Os vídeos que o mostraram mantêm seus retratos; a pessoa do elenco passa a ser do próprio vídeo.

<a id="save"></a>
## Salvar uma pessoa do vídeo no mundo

Uma pessoa do elenco de um vídeo pode virar personagem: na etapa [Cenas](scenes.md#cast), **Salvar em Mundo** a guarda no mundo do projeto e **Salvar na biblioteca global** na biblioteca global, com o retrato dela no visual do vídeo. A pessoa do vídeo passa a ser esse personagem.

<a id="backup"></a>
## Backups e o arquivo de configurações

Os [backups](backup.md) levam os personagens e suas imagens. O [arquivo de configurações](settings.md) não: ele leva preferências, não o seu trabalho. Excluir um projeto exclui os personagens dele.
