---
id: templates
title: Modelos
group: production
place: templates
tour: templates
---

# Modelos

Os modelos são os prompts que o Bardo envia à IA. Mude um para mudar como o Bardo escreve roteiros, planeja cenas, redige prompts de música ou prepara publicações, para todos os canais. Eles ficam em [Modelos](bardo:go/templates). [Mostre a tela para mim](bardo:tour/templates).

<a id="kinds"></a>
## Tipos de modelo

Há um modelo para cada tipo de geração:

- **Roteiro**: o roteiro do vídeo ([Roteiro](script.md)).
- **Prompts de imagem**: o plano de cenas e o prompt de imagem de cada cena ([Cenas e imagens](scenes.md#plan)).
- **Prompt de música**: o prompt para a sua ferramenta de música ([Roteiro](script.md#music)).
- **Metadados**: o título, a descrição e as tags de cada rede, na etapa Publicação.
- **Miniatura**: o texto da miniatura do YouTube de um vídeo longo e o prompt do fundo dela ([Miniatura](uploading.md#thumbnail)).
- **Capítulos**: os capítulos do YouTube de um vídeo longo, a partir das frases da narração ([Capítulos](uploading.md#chapters)).

<a id="versions"></a>
## Versões

Cada vez que você salva, entra uma versão, numerada em ordem; a mais nova é a **Atual** e é a que as gerações usam. As versões anteriores continuam na lista: escolha uma para carregar o texto dela no editor, e salve para torná-la a atual de novo. Cada geração registra a versão que usou, então os **Detalhes** de um roteiro dizem qual versão o escreveu.

Quando uma atualização do Bardo melhora um padrão que você nunca mudou, o padrão novo entra como a próxima versão. Um modelo que você editou fica como você deixou; **Carregar o padrão do Bardo** mostra o que mudou.

<a id="fields"></a>
## Instruções e prompt

Um modelo tem duas partes, cada uma com até 8.000 caracteres:

- **Instruções**: as regras fixas, como o papel, o tom e o formato da resposta.
- **Prompt**: a tarefa em si, com os dados do projeto colocados por meio de variáveis. Ele não pode ficar vazio.

<a id="variables"></a>
## Variáveis

Escreva uma variável como `{{nome}}`; cada geração a troca pelo valor do projeto. A lista sob o editor mostra as variáveis que este tipo de modelo pode usar e o que cada uma traz, como `{{channel_name}}`, `{{persona}}` ou `{{narration_sentences}}`. `{{video_title}}` é o título do YouTube quando já foi escrito, senão o do projeto. `{{target_length}}` e `{{frame_shape}}` seguem o [formato](projects.md#format) do vídeo: o tamanho do roteiro e o formato das imagens. Uma variável escrita errado ou desconhecida, ou sem o `}}` de fechamento, impede que o modelo seja salvo.

<a id="save"></a>
## Salvar e recomeçar

**Salvar como nova versão** guarda o seu texto como a próxima versão; quando nada mudou, nenhuma versão é criada. **Descartar alterações** volta à versão atual. **Carregar o padrão do Bardo** põe no editor o texto com que o Bardo veio, para salvar como nova versão se você quiser de volta.
